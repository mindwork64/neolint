//! Language Server Protocol adapter for the existing NeoLint analysis API.
//!
//! The server deliberately owns no parser or semantic rules. Each open LSL
//! document is prepared through [`AnalysisRun`] and analyzed through
//! [`crate::source::AnalysisUnit::analyze_with_config`], the same configured entry point used
//! by library consumers.

use crate::ast::TopLevel;
use crate::ast_semantic::scopes::{type_name, ScopeTree, Symbol, SymbolKind};
use crate::builtins::{self, Signature};
use crate::cancellation::AnalysisCancellationToken;
use crate::config::LintConfig;
use crate::diagnostic::{Severity, Span};
use crate::engine::{AnalysisEngine, EngineAnalysisOptions};
use crate::pipeline::{AnalysisRun, CancellableSetupError};
use crate::preprocessor::PreprocessSettings;
use crate::source::manager::{normalize_lexical_path, normalize_local_path};
use crate::source::unit::CancellableAnalysisError;
use crate::source::{SourceFile, SourceFileKind, SourceSpan};
use crossbeam_channel::{select, Receiver, Sender};
use lsp_server::{Connection, Message, Notification, Response, ResponseError};
use lsp_types::{
    CreateFilesParams, DeleteFilesParams, Diagnostic as LspDiagnostic,
    DiagnosticRelatedInformation, DiagnosticSeverity, FileOperationFilter, FileOperationPattern,
    FileOperationRegistrationOptions, Hover, HoverContents, HoverProviderCapability,
    InitializeParams, Location, MarkupContent, MarkupKind, MessageType, OneOf, Position,
    PublishDiagnosticsParams, Range, RenameFilesParams, ServerCapabilities,
    TextDocumentPositionParams, TextDocumentSyncCapability, TextDocumentSyncKind,
    TextDocumentSyncOptions, Url, WorkspaceFileOperationsServerCapabilities,
    WorkspaceServerCapabilities,
};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet, VecDeque};
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};

const SERVER_NAME: &str = "NeoLint";

/// Run the NeoLint language server over standard input/output.
///
/// Standard output is reserved for LSP frames. Logs and fatal errors go to
/// standard error so they cannot corrupt the JSON-RPC transport.
pub fn run_stdio() -> Result<(), Box<dyn Error + Send + Sync>> {
    let (connection, io_threads) = Connection::stdio();
    let (initialize_id, initialize_value) = connection.initialize_start()?;
    let initialize_params: InitializeParams = serde_json::from_value(initialize_value)?;
    let options =
        LspInitializationOptions::from_value(initialize_params.initialization_options.clone());
    let workspace_root = workspace_root(&initialize_params);
    let (config, config_error) =
        load_config(workspace_root.as_deref(), options.config_path.as_deref());

    let file_operations = FileOperationRegistrationOptions {
        filters: vec![FileOperationFilter {
            scheme: Some("file".to_owned()),
            pattern: FileOperationPattern {
                glob: "**/*".to_owned(),
                // Omit matches to subscribe to both file and folder operations.
                matches: None,
                options: None,
            },
        }],
    };
    let client_file_operations = initialize_params
        .capabilities
        .workspace
        .as_ref()
        .and_then(|workspace| workspace.file_operations.as_ref());
    let server_file_operations =
        client_file_operations.map(|client| WorkspaceFileOperationsServerCapabilities {
            did_create: client
                .did_create
                .unwrap_or(false)
                .then(|| file_operations.clone()),
            did_rename: client
                .did_rename
                .unwrap_or(false)
                .then(|| file_operations.clone()),
            did_delete: client
                .did_delete
                .unwrap_or(false)
                .then(|| file_operations.clone()),
            ..Default::default()
        });
    let workspace = server_file_operations
        .filter(|operations| {
            operations.did_create.is_some()
                || operations.did_rename.is_some()
                || operations.did_delete.is_some()
        })
        .map(|file_operations| WorkspaceServerCapabilities {
            file_operations: Some(file_operations),
            ..Default::default()
        });
    let capabilities = ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Options(
            TextDocumentSyncOptions {
                open_close: Some(true),
                change: Some(TextDocumentSyncKind::FULL),
                save: Some(lsp_types::TextDocumentSyncSaveOptions::Supported(true)),
                ..Default::default()
            },
        )),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        definition_provider: Some(OneOf::Left(true)),
        workspace,
        ..Default::default()
    };
    connection.initialize_finish(initialize_id, serde_json::to_value(capabilities)?)?;

    if let Some(error) = &config_error {
        send_show_message(
            &connection,
            MessageType::ERROR,
            &format!("NeoLint configuration: {error}"),
        )?;
    } else {
        eprintln!("{SERVER_NAME} LSP initialized");
    }

    let mut state = ServerState {
        config,
        documents: Arc::new(HashMap::new()),
        published_by_root: HashMap::new(),
        symbols_by_root: HashMap::new(),
        dependencies_by_root: HashMap::new(),
        dependency_tracking_incomplete_by_root: HashSet::new(),
        next_revision: 0,
        file_operation_revision: 0,
    };
    let mut worker = AnalysisWorker::new()?;

    loop {
        select! {
            recv(connection.receiver) -> message => {
                let Ok(message) = message else {
                    break;
                };
                match message {
                    Message::Request(request) => {
                        if connection.handle_shutdown(&request)? {
                            break;
                        }
                        let response = handle_request(request, &state);
                        connection.sender.send(Message::Response(response))?;
                    }
                    Message::Notification(notification) => {
                        if notification.method == "exit" {
                            break;
                        }
                        handle_notification(&connection, &mut state, &worker, notification)?;
                    }
                    Message::Response(_) => {}
                }
            }
            recv(worker.results) -> result => {
                match result {
                    Ok(result) => {
                        handle_analysis_result(&connection, &mut state, &worker, result)?;
                    }
                    Err(_) => {
                        eprintln!("{SERVER_NAME} analysis worker stopped unexpectedly");
                        break;
                    }
                }
            }
        }
    }

    // Pending work is discarded and the active analysis receives a cancellation
    // signal. The worker is detached so shutdown never waits on disk preprocessing.
    worker.stop();
    drop(state);
    drop(connection);
    io_threads.join()?;
    eprintln!("{SERVER_NAME} LSP stopped");
    Ok(())
}

#[derive(Debug, Default)]
struct LspInitializationOptions {
    config_path: Option<PathBuf>,
}

impl LspInitializationOptions {
    fn from_value(value: Option<Value>) -> Self {
        let config_path = value
            .as_ref()
            .and_then(|value| value.get("configPath"))
            .and_then(Value::as_str)
            .filter(|path| !path.trim().is_empty())
            .map(PathBuf::from);
        Self { config_path }
    }
}

#[derive(Clone, Debug)]
struct DocumentState {
    /// Immutable source storage is shared by live state and every queued task.
    text: Arc<str>,
    version: i32,
    /// Server-local revision distinguishes snapshots even when an LSP save
    /// updates text without changing the client-supplied document version.
    revision: u64,
}

type RootDiagnostics = HashMap<Url, Vec<LspDiagnostic>>;

struct ServerState {
    config: Option<LintConfig>,
    /// Copy-on-write snapshots let queued roots share one document map.
    documents: Arc<HashMap<Url, DocumentState>>,
    /// Per-root diagnostic snapshots, keyed by the URI they belong to.
    /// A single included URI may be present in several root snapshots.
    published_by_root: HashMap<Url, RootDiagnostics>,
    /// Parsed symbol metadata is replaced atomically with each accepted root
    /// analysis and discarded as soon as a newer analysis is scheduled.
    symbols_by_root: HashMap<Url, CachedScriptSymbols>,
    /// Source documents actually loaded by each root's most recent analysis.
    dependencies_by_root: HashMap<Url, HashSet<PathBuf>>,
    /// Roots whose dependency index exceeded its path budget; changes to any
    /// open local document conservatively reanalyze these roots.
    dependency_tracking_incomplete_by_root: HashSet<Url>,
    next_revision: u64,
    /// Global filesystem-operation generation. Any result started before a
    /// create/rename/delete notification is stale, even if the worker has
    /// already finished and its result is waiting for the LSP thread.
    file_operation_revision: u64,
}

struct AnalysisTask {
    root_uri: Url,
    root_version: i32,
    root_revision: u64,
    file_operation_revision: u64,
    documents: Arc<HashMap<Url, DocumentState>>,
    config: LintConfig,
}

struct AnalysisResult {
    root_uri: Url,
    root_version: i32,
    root_revision: u64,
    file_operation_revision: u64,
    dependencies: HashSet<PathBuf>,
    dependencies_complete: bool,
    dependency_revisions: HashMap<PathBuf, HashMap<Url, u64>>,
    diagnostics: Result<RootDiagnostics, String>,
    symbols: Option<Arc<ScriptSymbolIndex>>,
}

struct AnalyzedDocument {
    diagnostics: RootDiagnostics,
    dependencies: HashSet<PathBuf>,
    dependencies_complete: bool,
    symbols: Arc<ScriptSymbolIndex>,
}

struct CachedScriptSymbols {
    version: i32,
    revision: u64,
    index: Arc<ScriptSymbolIndex>,
}

struct ScriptSymbolIndex {
    /// Identifier occurrences mapped back to the open root buffer. Building
    /// these from the prepared AST keeps inactive preprocessor branches out of
    /// editor lookup and gives each occurrence its effective source mapping.
    occurrences: Vec<SymbolOccurrence>,
}

struct SymbolOccurrence {
    span: Span,
    hover: String,
    definition: Option<Span>,
}

struct RunningAnalysis {
    root_uri: Url,
    cancellation: AnalysisCancellationToken,
}

#[derive(Default)]
struct PendingAnalyses {
    tasks: HashMap<Url, AnalysisTask>,
    order: VecDeque<Url>,
    running: Option<RunningAnalysis>,
    stopped: bool,
}

/// A single background worker with a latest-task-per-root pending queue.
///
/// New edits replace queued work for the same root. One task may already be
/// running; its result is accepted only while the root and its dependency-path
/// revision signatures still match the live document state.
struct AnalysisWorker {
    pending: Arc<(Mutex<PendingAnalyses>, Condvar)>,
    results: Receiver<AnalysisResult>,
    thread: Option<JoinHandle<()>>,
}

impl AnalysisWorker {
    fn new() -> Result<Self, std::io::Error> {
        let pending = Arc::new((Mutex::new(PendingAnalyses::default()), Condvar::new()));
        let worker_pending = Arc::clone(&pending);
        let (result_sender, results) = crossbeam_channel::unbounded();

        let thread = thread::Builder::new()
            .name("neolint-lsp-analysis".to_owned())
            .spawn(move || {
                analysis_worker_loop(worker_pending, result_sender);
            })?;

        Ok(Self {
            pending,
            results,
            thread: Some(thread),
        })
    }

    fn schedule_many(
        &self,
        root_uris: impl IntoIterator<Item = Url>,
        documents: Arc<HashMap<Url, DocumentState>>,
        config: LintConfig,
        file_operation_revision: u64,
    ) {
        let tasks: Vec<_> = root_uris
            .into_iter()
            .filter_map(|uri| {
                make_analysis_task(
                    uri,
                    Arc::clone(&documents),
                    config.clone(),
                    file_operation_revision,
                )
            })
            .collect();
        if tasks.is_empty() {
            return;
        }
        let (lock, ready) = &*self.pending;
        let mut pending = lock.lock().expect("analysis queue mutex poisoned");
        if schedule_pending_tasks(&mut pending, tasks) {
            ready.notify_one();
        }
    }

    /// Return roots that are queued or currently being analyzed. A workspace
    /// file operation conservatively refreshes these roots because their
    /// dependency snapshots may not yet describe the filesystem they observe.
    fn roots_in_flight(&self) -> HashSet<Url> {
        let (lock, _) = &*self.pending;
        let pending = lock.lock().expect("analysis queue mutex poisoned");
        pending_and_running_roots(&pending)
    }

    fn close_root(&self, root_uri: &Url) {
        let (lock, ready) = &*self.pending;
        let mut pending = lock.lock().expect("analysis queue mutex poisoned");
        pending.tasks.remove(root_uri);
        pending.order.retain(|uri| uri != root_uri);
        if let Some(running) = &pending.running {
            if &running.root_uri == root_uri {
                running.cancellation.cancel();
            }
        }
        ready.notify_one();
    }

    fn stop(&mut self) {
        let (lock, ready) = &*self.pending;
        let mut pending = lock.lock().expect("analysis queue mutex poisoned");
        pending.stopped = true;
        pending.tasks.clear();
        pending.order.clear();
        if let Some(running) = &pending.running {
            running.cancellation.cancel();
        }
        ready.notify_one();
        drop(pending);
        // Dropping JoinHandle detaches a currently running analysis. The
        // worker owns no connection state and exits when that analysis ends.
        let _ = self.thread.take();
    }
}

impl Drop for AnalysisWorker {
    fn drop(&mut self) {
        self.stop();
    }
}

fn make_analysis_task(
    root_uri: Url,
    documents: Arc<HashMap<Url, DocumentState>>,
    config: LintConfig,
    file_operation_revision: u64,
) -> Option<AnalysisTask> {
    let root = documents.get(&root_uri)?;
    Some(AnalysisTask {
        root_uri,
        root_version: root.version,
        root_revision: root.revision,
        file_operation_revision,
        documents,
        config,
    })
}

fn pending_and_running_roots(pending: &PendingAnalyses) -> HashSet<Url> {
    let mut roots: HashSet<_> = pending.tasks.keys().cloned().collect();
    if let Some(running) = &pending.running {
        roots.insert(running.root_uri.clone());
    }
    roots
}

fn schedule_pending_tasks(pending: &mut PendingAnalyses, tasks: Vec<AnalysisTask>) -> bool {
    let targeted_roots: HashSet<_> = tasks.iter().map(|task| task.root_uri.clone()).collect();
    if let Some(running) = &pending.running {
        // Only interrupt active work when one of its own roots is affected.
        if targeted_roots.contains(&running.root_uri) {
            running.cancellation.cancel();
        }
    }
    let mut scheduled = false;
    for task in tasks {
        scheduled |= enqueue_pending(pending, task);
    }
    scheduled
}

fn affected_roots_for_document(state: &ServerState, changed_uri: &Url) -> Vec<Url> {
    let mut roots = HashSet::new();
    if is_lsl_document(changed_uri) && state.documents.contains_key(changed_uri) {
        roots.insert(changed_uri.clone());
    }
    let changed_paths = changed_uri
        .to_file_path()
        .ok()
        .map(|path| [normalize_path(&path), normalize_lexical_path(&path)]);
    for (root_uri, dependencies) in &state.dependencies_by_root {
        if !state.documents.contains_key(root_uri) || changed_paths.is_none() {
            continue;
        }
        if state
            .dependency_tracking_incomplete_by_root
            .contains(root_uri)
            || changed_paths.as_ref().is_some_and(|paths| {
                dependencies.iter().any(|dependency| {
                    paths
                        .iter()
                        .any(|path| dependency == path || dependency.starts_with(path))
                })
            })
        {
            roots.insert(root_uri.clone());
        }
    }
    let mut roots: Vec<_> = roots.into_iter().collect();
    roots.sort_by(|left, right| left.as_str().cmp(right.as_str()));
    roots
}

fn enqueue_pending(pending: &mut PendingAnalyses, task: AnalysisTask) -> bool {
    if pending.stopped {
        return false;
    }
    if !pending.tasks.contains_key(&task.root_uri) {
        pending.order.push_back(task.root_uri.clone());
    }
    pending.tasks.insert(task.root_uri.clone(), task);
    true
}

fn dependency_revision_snapshot(
    dependencies: &HashSet<PathBuf>,
    documents: &HashMap<Url, DocumentState>,
    dependencies_complete: bool,
) -> HashMap<PathBuf, HashMap<Url, u64>> {
    let mut open_by_path = HashMap::<PathBuf, HashMap<Url, u64>>::new();
    for (uri, document) in documents {
        if let Ok(path) = uri.to_file_path() {
            for identity in [normalize_path(&path), normalize_lexical_path(&path)] {
                open_by_path
                    .entry(identity)
                    .or_default()
                    .insert(uri.clone(), document.revision);
            }
        }
    }
    if !dependencies_complete {
        return open_by_path;
    }
    dependencies
        .iter()
        .map(|path| (path.clone(), open_by_path.remove(path).unwrap_or_default()))
        .collect()
}

fn analysis_snapshot_is_current(state: &ServerState, result: &AnalysisResult) -> bool {
    state.file_operation_revision == result.file_operation_revision
        && state
            .documents
            .get(&result.root_uri)
            .is_some_and(|document| {
                document.version == result.root_version && document.revision == result.root_revision
            })
        && dependency_revision_snapshot(
            &result.dependencies,
            &state.documents,
            result.dependencies_complete,
        ) == result.dependency_revisions
}

fn analysis_worker_loop(
    pending: Arc<(Mutex<PendingAnalyses>, Condvar)>,
    result_sender: Sender<AnalysisResult>,
) {
    loop {
        let (task, cancellation) = {
            let (lock, ready) = &*pending;
            let mut queue = lock.lock().expect("analysis queue mutex poisoned");
            loop {
                if queue.stopped {
                    return;
                }
                if let Some(uri) = queue.order.pop_front() {
                    if let Some(task) = queue.tasks.remove(&uri) {
                        let cancellation = AnalysisCancellationToken::new();
                        queue.running = Some(RunningAnalysis {
                            root_uri: task.root_uri.clone(),
                            cancellation: cancellation.clone(),
                        });
                        break (task, cancellation);
                    }
                } else {
                    queue = ready.wait(queue).expect("analysis queue mutex poisoned");
                }
            }
        };

        let result = analyze_document(
            &task.root_uri,
            task.documents
                .get(&task.root_uri)
                .expect("analysis task root document"),
            &task.config,
            task.documents.as_ref(),
            &cancellation,
        );
        let was_cancelled =
            cancellation.is_cancelled() || matches!(result, Err(AnalyzeDocumentError::Cancelled));
        {
            let (lock, _) = &*pending;
            let mut queue = lock.lock().expect("analysis queue mutex poisoned");
            queue.running = None;
        }
        let was_cancelled = was_cancelled || cancellation.is_cancelled();
        if was_cancelled {
            continue;
        }
        let (dependencies, dependencies_complete, diagnostics, symbols) = match result {
            Ok(analysis) => (
                analysis.dependencies,
                analysis.dependencies_complete,
                Ok(analysis.diagnostics),
                Some(analysis.symbols),
            ),
            Err(AnalyzeDocumentError::Failure(error)) => {
                let dependencies = task
                    .root_uri
                    .to_file_path()
                    .ok()
                    .map(|path| HashSet::from([normalize_path(&path)]))
                    .unwrap_or_default();
                (dependencies, false, Err(error), None)
            }
            Err(AnalyzeDocumentError::Cancelled) => continue,
        };
        let dependency_revisions = dependency_revision_snapshot(
            &dependencies,
            task.documents.as_ref(),
            dependencies_complete,
        );
        if result_sender
            .send(AnalysisResult {
                root_uri: task.root_uri,
                root_version: task.root_version,
                root_revision: task.root_revision,
                file_operation_revision: task.file_operation_revision,
                dependencies,
                dependencies_complete,
                dependency_revisions,
                diagnostics,
                symbols,
            })
            .is_err()
        {
            return;
        }
    }
}

fn is_newer_version(current: i32, incoming: i32) -> bool {
    incoming > current
}

#[allow(deprecated)]
fn workspace_root(params: &InitializeParams) -> Option<PathBuf> {
    if let Some(folders) = &params.workspace_folders {
        if let Some(folder) = folders.first() {
            if let Ok(path) = folder.uri.to_file_path() {
                return Some(path);
            }
        }
    }
    params
        .root_uri
        .as_ref()
        .and_then(|uri| uri.to_file_path().ok())
        .or_else(|| params.root_path.as_ref().map(PathBuf::from))
}

fn load_config(
    root: Option<&Path>,
    explicit_path: Option<&Path>,
) -> (Option<LintConfig>, Option<String>) {
    let selected = explicit_path
        .map(|path| {
            if path.is_absolute() {
                path.to_path_buf()
            } else if let Some(root) = root {
                root.join(path)
            } else {
                path.to_path_buf()
            }
        })
        .or_else(|| {
            root.map(|path| path.join(".neolint.conf"))
                .filter(|path| path.is_file())
        });

    let mut config = if let Some(path) = selected {
        match load_config_file(&path) {
            Ok(config) => config,
            Err(error) => return (None, Some(error)),
        }
    } else {
        LintConfig::default()
    };

    // Firestorm opens a temporary copy of the script, so its containing
    // directory is not necessarily the project root. Keep configured include
    // directories first, then use the workspace root as the final fallback.
    if config.preprocess_enabled() {
        if let Some(root) = root {
            let workspace_path = normalize_path(root);
            let mut include_dirs = config.include_dirs().to_vec();
            if !include_dirs
                .iter()
                .any(|directory| normalize_path(directory) == workspace_path)
            {
                include_dirs.push(workspace_path);
            }
            config.set_include_dirs(include_dirs);
        }
    }
    if let Err(error) = validate_effective_config(&config) {
        return (None, Some(error));
    }
    (Some(config), None)
}

fn load_config_file(path: &Path) -> Result<LintConfig, String> {
    let source = fs::read_to_string(path)
        .map_err(|error| format!("cannot read '{}': {error}", path.display()))?;
    let mut config = LintConfig::parse(&source)
        .map_err(|error| format!("invalid config '{}': {error}", path.display()))?;
    if !config.include_dirs().is_empty() {
        let base = path.parent().unwrap_or_else(|| Path::new("."));
        let include_dirs = config
            .include_dirs()
            .iter()
            .map(|directory| {
                if directory.is_absolute() {
                    directory.clone()
                } else {
                    base.join(directory)
                }
            })
            .collect();
        config.set_include_dirs(include_dirs);
    }
    validate_effective_config(&config)?;
    Ok(config)
}

fn validate_effective_config(config: &LintConfig) -> Result<(), String> {
    config.validate_target_settings()?;
    if config.target_explicit() && config.engine() == AnalysisEngine::Legacy {
        return Err(
            "target profiles require the AST or Compare engine; the frozen Legacy engine is not target-profile aware"
                .to_owned(),
        );
    }
    if !config.preprocess_enabled() && !config.include_dirs().is_empty() {
        return Err("include directories require --preprocess or --dialect firestorm".to_owned());
    }
    for directory in config.include_dirs() {
        match fs::metadata(directory) {
            Ok(metadata) if metadata.is_dir() => {}
            Ok(_) => {
                return Err(format!(
                    "include path '{}' is not a directory",
                    directory.display()
                ));
            }
            Err(error) => {
                return Err(format!(
                    "cannot access include directory '{}': {error}",
                    directory.display()
                ));
            }
        }
    }
    Ok(())
}

fn handle_notification(
    connection: &Connection,
    state: &mut ServerState,
    worker: &AnalysisWorker,
    notification: Notification,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    match notification.method.as_str() {
        "textDocument/didOpen" => {
            let params: lsp_types::DidOpenTextDocumentParams =
                serde_json::from_value(notification.params)?;
            let document = params.text_document;
            if document.uri.scheme() != "file" {
                return Ok(());
            }
            state.next_revision = state.next_revision.saturating_add(1);
            Arc::make_mut(&mut state.documents).insert(
                document.uri.clone(),
                DocumentState {
                    text: Arc::from(document.text),
                    version: document.version,
                    revision: state.next_revision,
                },
            );
            schedule_affected_roots(connection, state, worker, &document.uri)?;
        }
        "textDocument/didChange" => {
            let params: lsp_types::DidChangeTextDocumentParams =
                serde_json::from_value(notification.params)?;
            let uri = params.text_document.uri;
            let Some(change) = params.content_changes.into_iter().last() else {
                return Ok(());
            };
            let is_newer = state.documents.get(&uri).is_some_and(|document| {
                is_newer_version(document.version, params.text_document.version)
            });
            if !is_newer {
                return Ok(());
            }
            state.next_revision = state.next_revision.saturating_add(1);
            if let Some(document) = Arc::make_mut(&mut state.documents).get_mut(&uri) {
                document.text = Arc::from(change.text);
                document.version = params.text_document.version;
                document.revision = state.next_revision;
            }
            schedule_affected_roots(connection, state, worker, &uri)?;
        }
        "textDocument/didSave" => {
            let params: lsp_types::DidSaveTextDocumentParams =
                serde_json::from_value(notification.params)?;
            let uri = params.text_document.uri;
            let changed = params
                .text
                .as_ref()
                .and_then(|text| {
                    state
                        .documents
                        .get(&uri)
                        .map(|document| document.text.as_ref() != text.as_str())
                })
                .unwrap_or(false);
            if changed {
                state.next_revision = state.next_revision.saturating_add(1);
                if let (Some(text), Some(document)) = (
                    params.text,
                    Arc::make_mut(&mut state.documents).get_mut(&uri),
                ) {
                    document.text = Arc::from(text);
                    document.revision = state.next_revision;
                }
            }
            if state.documents.contains_key(&uri) {
                schedule_affected_roots(connection, state, worker, &uri)?;
            }
        }
        "workspace/didCreateFiles" => {
            let params: CreateFilesParams = serde_json::from_value(notification.params)?;
            let uris = params
                .files
                .into_iter()
                .filter_map(|file| Url::parse(&file.uri).ok());
            schedule_file_operation_roots(connection, state, worker, uris)?;
        }
        "workspace/didRenameFiles" => {
            let params: RenameFilesParams = serde_json::from_value(notification.params)?;
            let uris = params.files.into_iter().flat_map(|file| {
                [
                    Url::parse(&file.old_uri).ok(),
                    Url::parse(&file.new_uri).ok(),
                ]
                .into_iter()
                .flatten()
            });
            schedule_file_operation_roots(connection, state, worker, uris)?;
        }
        "workspace/didDeleteFiles" => {
            let params: DeleteFilesParams = serde_json::from_value(notification.params)?;
            let uris: Vec<_> = params
                .files
                .into_iter()
                .filter_map(|file| Url::parse(&file.uri).ok())
                .collect();
            schedule_file_operation_roots(connection, state, worker, uris)?;
        }
        "textDocument/didClose" => {
            let params: lsp_types::DidCloseTextDocumentParams =
                serde_json::from_value(notification.params)?;
            let was_tracked =
                remove_tracked_document(&mut state.documents, &params.text_document.uri);
            if was_tracked {
                let closed_uri = params.text_document.uri;
                let mut affected = affected_roots_for_document(state, &closed_uri);
                affected.retain(|root| root != &closed_uri);
                worker.close_root(&closed_uri);
                state.symbols_by_root.remove(&closed_uri);
                state.dependencies_by_root.remove(&closed_uri);
                state
                    .dependency_tracking_incomplete_by_root
                    .remove(&closed_uri);
                schedule_roots(connection, state, worker, affected)?;
                clear_root_diagnostics(connection, state, &closed_uri)?;
            } else {
                clear_root_diagnostics(connection, state, &params.text_document.uri)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn schedule_file_operation_roots(
    connection: &Connection,
    state: &mut ServerState,
    worker: &AnalysisWorker,
    changed_uris: impl IntoIterator<Item = Url>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    // Advance before selecting roots. Results produced against the previous
    // filesystem generation must be discarded even when their dependency
    // scan has not populated dependencies_by_root yet (or the worker has
    // cleared `running` but the main thread has not accepted its result).
    state.file_operation_revision = state.file_operation_revision.saturating_add(1);

    // Requeue pending and active roots immediately. The generation check below
    // remains necessary for the narrow completed-but-not-yet-accepted window.
    let mut roots = worker.roots_in_flight();
    for uri in changed_uris {
        if uri.scheme() == "file" {
            let affected = affected_roots_for_document(state, &uri);
            roots.extend(affected);
        }
    }
    let mut roots: Vec<_> = roots.into_iter().collect();
    roots.sort_by(|left, right| left.as_str().cmp(right.as_str()));
    schedule_roots(connection, state, worker, roots)
}

fn remove_tracked_document(documents: &mut Arc<HashMap<Url, DocumentState>>, uri: &Url) -> bool {
    Arc::make_mut(documents).remove(uri).is_some()
}

fn is_lsl_document(uri: &Url) -> bool {
    uri.scheme() == "file" && uri.path().to_ascii_lowercase().ends_with(".lsl")
}

fn schedule_affected_roots(
    connection: &Connection,
    state: &mut ServerState,
    worker: &AnalysisWorker,
    changed_uri: &Url,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let roots = affected_roots_for_document(state, changed_uri);
    schedule_roots(connection, state, worker, roots)
}

fn schedule_roots(
    connection: &Connection,
    state: &mut ServerState,
    worker: &AnalysisWorker,
    roots: Vec<Url>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    if roots.is_empty() {
        return Ok(());
    }
    for root_uri in &roots {
        state.symbols_by_root.remove(root_uri);
    }
    let Some(config) = &state.config else {
        for root_uri in roots {
            if state.documents.contains_key(&root_uri) {
                publish_for_root(connection, state, &root_uri, HashMap::new())?;
            }
        }
        return Ok(());
    };
    worker.schedule_many(
        roots,
        Arc::clone(&state.documents),
        config.clone(),
        state.file_operation_revision,
    );
    Ok(())
}

fn handle_analysis_result(
    connection: &Connection,
    state: &mut ServerState,
    worker: &AnalysisWorker,
    result: AnalysisResult,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    if !analysis_snapshot_is_current(state, &result) {
        // A newer document snapshot arrived while the task was running.
        // Replacing the pending task coalesces any intermediate edits.
        schedule_roots(connection, state, worker, vec![result.root_uri.clone()])?;
        return Ok(());
    }

    match result.diagnostics {
        Ok(diagnostics) => {
            if let Some(index) = result.symbols {
                state.symbols_by_root.insert(
                    result.root_uri.clone(),
                    CachedScriptSymbols {
                        version: result.root_version,
                        revision: result.root_revision,
                        index,
                    },
                );
            }
            state
                .dependencies_by_root
                .insert(result.root_uri.clone(), result.dependencies);
            if result.dependencies_complete {
                state
                    .dependency_tracking_incomplete_by_root
                    .remove(&result.root_uri);
            } else {
                state
                    .dependency_tracking_incomplete_by_root
                    .insert(result.root_uri.clone());
            }
            publish_for_root(connection, state, &result.root_uri, diagnostics)?;
        }
        Err(error) => {
            // Keep the last successful dependency set. A failed disk read or
            // preprocessing attempt must not erase the edges that let a later
            // include edit recover this root.
            state
                .dependencies_by_root
                .entry(result.root_uri.clone())
                .or_insert_with(|| {
                    result
                        .root_uri
                        .to_file_path()
                        .ok()
                        .map(|path| HashSet::from([normalize_path(&path)]))
                        .unwrap_or_default()
                });
            // The failed attempt may not have completed include discovery.
            // Preserve the last successful edges but fall back to watching all
            // local file operations until a later analysis succeeds.
            state
                .dependency_tracking_incomplete_by_root
                .insert(result.root_uri.clone());
            eprintln!("NeoLint analysis failed for {}: {error}", result.root_uri);
            publish_for_root(connection, state, &result.root_uri, HashMap::new())?;
            send_show_message(
                connection,
                MessageType::WARNING,
                &format!("NeoLint could not analyze this document: {error}"),
            )?;
        }
    }
    Ok(())
}

#[derive(Debug)]
enum AnalyzeDocumentError {
    Cancelled,
    Failure(String),
}

impl From<String> for AnalyzeDocumentError {
    fn from(error: String) -> Self {
        Self::Failure(error)
    }
}

fn analyze_document(
    root_uri: &Url,
    document: &DocumentState,
    config: &LintConfig,
    open_documents: &HashMap<Url, DocumentState>,
    cancellation: &AnalysisCancellationToken,
) -> Result<AnalyzedDocument, AnalyzeDocumentError> {
    if cancellation.is_cancelled() {
        return Err(AnalyzeDocumentError::Cancelled);
    }
    let root_path = root_uri.to_file_path().map_err(|_| {
        AnalyzeDocumentError::Failure(format!("unsupported document URI: {root_uri}"))
    })?;
    let display_name = root_path.to_string_lossy().into_owned();

    let mut open_documents_by_uri: Vec<_> = open_documents.iter().collect();
    open_documents_by_uri.sort_by(|(left, _), (right, _)| left.as_str().cmp(right.as_str()));
    let mut open_paths = HashMap::<PathBuf, Url>::new();
    let mut include_overlays = HashMap::<PathBuf, Arc<str>>::new();
    for (uri, open_document) in open_documents_by_uri {
        if cancellation.is_cancelled() {
            return Err(AnalyzeDocumentError::Cancelled);
        }
        if let Ok(path) = uri.to_file_path() {
            let path = normalize_path(&path);
            // Resolve URI aliases deterministically for both buffer text and source mapping.
            if !open_paths.contains_key(&path) {
                open_paths.insert(path.clone(), uri.clone());
                include_overlays.insert(path, Arc::clone(&open_document.text));
            }
        }
    }

    let mut run =
        AnalysisRun::new().map_err(|error| AnalyzeDocumentError::Failure(error.to_string()))?;
    let unit_index = if config.preprocess_enabled() {
        let settings = PreprocessSettings::full()
            .with_dialect(config.dialect())
            .with_include_dirs(config.include_dirs().to_vec());
        run.add_preprocessed_text_cancellable_with_overlays(
            &display_name,
            &document.text,
            settings,
            include_overlays,
            cancellation,
        )
        .map_err(|error| match error {
            CancellableSetupError::Cancelled => AnalyzeDocumentError::Cancelled,
            CancellableSetupError::Setup(error) => AnalyzeDocumentError::Failure(error.to_string()),
        })?
    } else if config.engine() == AnalysisEngine::Legacy {
        run.add_legacy_text(&display_name, &document.text)
            .map_err(|error| AnalyzeDocumentError::Failure(error.to_string()))?
    } else {
        run.add_prepared_text(&display_name, &document.text)
            .map_err(|error| AnalyzeDocumentError::Failure(error.to_string()))?
    };

    if cancellation.is_cancelled() {
        return Err(AnalyzeDocumentError::Cancelled);
    }

    let unit = run
        .unit(unit_index)
        .ok_or_else(|| "analysis unit disappeared after construction".to_owned())?;
    let analysis = unit
        .analyze_with_config_cancellable(config, &EngineAnalysisOptions::default(), cancellation)
        .map_err(|error| match error {
            CancellableAnalysisError::Cancelled => AnalyzeDocumentError::Cancelled,
            CancellableAnalysisError::Analysis(error) => {
                AnalyzeDocumentError::Failure(error.to_string())
            }
        })?;

    let dependencies = source_dependencies(
        root_uri,
        unit.manager().files(),
        unit.manager().include_dependencies(),
    );
    let dependencies_complete = unit.manager().include_dependencies_complete();

    let mut output = HashMap::<Url, Vec<LspDiagnostic>>::new();
    output.entry(root_uri.clone()).or_default();
    let root_source = unit.root();

    for rendered in analysis.diagnostics() {
        if cancellation.is_cancelled() {
            return Err(AnalyzeDocumentError::Cancelled);
        }
        let source_id = rendered.source();
        let Some(source_file) = unit.manager().file(source_id) else {
            continue;
        };
        let source_uri = if Some(source_id) == root_source {
            Some(root_uri.clone())
        } else {
            uri_for_open_source(source_file, &open_paths, open_documents)
        };
        let Some(source_uri) = source_uri else {
            continue;
        };
        let Some(open_source) = open_documents.get(&source_uri) else {
            continue;
        };
        // The preprocessor uses the selected immutable open-buffer overlay.
        // Keep this guard as a defensive check for sources not loaded via overlays.
        if source_file.text() != open_source.text.as_ref() {
            continue;
        }

        let span = rendered.diagnostic().span;
        let range = span_to_range(source_file.text(), span)?;
        let mut related_information = Vec::new();
        for related in rendered.related() {
            let Some(related_file) = unit.manager().file(related.span.source()) else {
                continue;
            };
            let related_uri = if Some(related.span.source()) == root_source {
                Some(root_uri.clone())
            } else {
                uri_for_open_source(related_file, &open_paths, open_documents)
            };
            let Some(related_uri) = related_uri else {
                continue;
            };
            let Some(related_document) = open_documents.get(&related_uri) else {
                continue;
            };
            if related_file.text() != related_document.text.as_ref() {
                continue;
            }
            let related_range = source_span_to_range(related_file.text(), related.span)?;
            related_information.push(DiagnosticRelatedInformation {
                location: Location {
                    uri: related_uri,
                    range: related_range,
                },
                message: related.label.clone(),
            });
        }

        let diagnostic = LspDiagnostic {
            range,
            severity: Some(map_severity(rendered.diagnostic().severity)),
            code: Some(lsp_types::NumberOrString::String(
                rendered.diagnostic().code.to_owned(),
            )),
            source: Some(SERVER_NAME.to_owned()),
            message: rendered.diagnostic().message.clone(),
            related_information: if related_information.is_empty() {
                None
            } else {
                Some(related_information)
            },
            ..Default::default()
        };
        output.entry(source_uri).or_default().push(diagnostic);
    }
    if cancellation.is_cancelled() {
        return Err(AnalyzeDocumentError::Cancelled);
    }
    let prepared = unit.prepared_source();
    let (symbol_source, parser_options) = prepared.map_or(
        (document.text.as_ref(), crate::ast::AstParseOptions::lsl()),
        |prepared| {
            (
                prepared.text(),
                crate::ast::AstParseOptions::from_flags(
                    prepared.switches_enabled(),
                    prepared.lazy_lists_enabled(),
                ),
            )
        },
    );
    let symbols = Arc::new(
        build_script_symbol_index(
            symbol_source,
            config,
            parser_options,
            cancellation,
            prepared,
            root_source,
        )
        .ok_or(AnalyzeDocumentError::Cancelled)?,
    );
    if cancellation.is_cancelled() {
        return Err(AnalyzeDocumentError::Cancelled);
    }
    Ok(AnalyzedDocument {
        diagnostics: output,
        dependencies,
        dependencies_complete,
        symbols,
    })
}

fn build_script_symbol_index(
    source: &str,
    config: &LintConfig,
    parser_options: crate::ast::AstParseOptions,
    cancellation: &AnalysisCancellationToken,
    prepared: Option<&crate::preprocessor::PreparedSource>,
    root_source: Option<crate::source::SourceId>,
) -> Option<ScriptSymbolIndex> {
    // The primary analysis and editor index must share one effective source:
    // inactive conditional branches are absent and macro expansions are
    // represented in the AST. SourceMap projects each indexed occurrence back
    // onto the exact open root buffer before it is exposed to the editor.
    let (tokens, _) = crate::lexer::lex_cancellable(source, cancellation)?;
    let (ast, _) =
        crate::ast::parse_ast_with_options_cancellable(&tokens, parser_options, cancellation)?;
    let scopes = ScopeTree::build_with_catalog(
        &ast,
        builtins::catalog_for_target(config.target()),
        config.target(),
    );
    let facts = crate::ast_semantic::expressions::collect_facts(&ast, &scopes);
    let mut function_signatures = HashMap::new();
    for item in &ast.items {
        if let TopLevel::Function(function) = item {
            let return_type = function
                .return_type
                .map(|ty| type_name(ty.kind))
                .unwrap_or("void");
            let parameters = function
                .parameters
                .iter()
                .map(|parameter| format!("{} {}", type_name(parameter.ty.kind), parameter.name))
                .collect::<Vec<_>>()
                .join(", ");
            function_signatures.insert(
                function.name_span.start,
                format!("{return_type} {}({parameters})", function.name),
            );
        }
    }
    for recovered in &ast.recovered_functions {
        function_signatures
            .entry(recovered.name_span.start)
            .or_insert_with(|| {
                format!(
                    "{} {}({})",
                    recovered.return_type,
                    recovered.name,
                    recovered.parameter_types.join(", ")
                )
            });
    }

    let mut occurrences = Vec::new();
    let mut occurrence_indices = HashMap::new();

    // Declarations themselves are valid hover/definition locations. A
    // declaration originating in an include can still provide contextual
    // hover text, but definition is deliberately limited to the open root.
    for scope in scopes.scopes() {
        for symbol in &scope.symbols {
            if cancellation.is_cancelled() {
                return None;
            }
            // Declarations in included files have no location in the currently
            // open root buffer, so keep navigation scoped to this script.
            if let Some(span) = map_index_span(prepared, root_source, symbol.span) {
                push_symbol_occurrence(
                    &mut occurrences,
                    &mut occurrence_indices,
                    SymbolOccurrence {
                        span,
                        hover: hover_for_user_symbol(&function_signatures, symbol),
                        definition: Some(span),
                    },
                );
            }
        }
    }

    // Resolve each parsed identifier reference using the same lexical scope
    // and declaration-order rules as semantic analysis. Component selectors
    // are not IdentifierUse nodes in the AST, so `position.x` cannot bind to an
    // unrelated variable named `x`.
    for usage in &facts.uses {
        if cancellation.is_cancelled() {
            return None;
        }
        let identifier = usage.identifier;
        let user_symbol = scopes
            .lookup_value_at(usage.scope, &identifier.name, identifier.span.start)
            .or_else(|| {
                scopes
                    .lookup(usage.scope, &identifier.name)
                    .filter(|symbol| symbol.kind == SymbolKind::Function)
            });
        let (hover, definition) = if let Some(symbol) = user_symbol {
            let definition = map_index_span(prepared, root_source, symbol.span);
            (
                hover_for_user_symbol(&function_signatures, symbol),
                definition,
            )
        } else {
            let Some(hover) = hover_for_builtin(&scopes, &identifier.name) else {
                continue;
            };
            (hover, None)
        };
        let Some(span) = map_index_span(prepared, root_source, identifier.span) else {
            continue;
        };
        push_symbol_occurrence(
            &mut occurrences,
            &mut occurrence_indices,
            SymbolOccurrence {
                span,
                hover,
                definition,
            },
        );
    }

    // Event names are declaration-like built-in uses, rather than expression
    // Identifier nodes, so index them explicitly from the parsed active AST.
    for item in &ast.items {
        let TopLevel::State(state) = item else {
            continue;
        };
        for event in &state.events {
            if cancellation.is_cancelled() {
                return None;
            }
            let Some(signature) = scopes.builtin_events().get(&event.name) else {
                continue;
            };
            let Some(span) = map_index_span(prepared, root_source, event.name_span) else {
                continue;
            };
            push_symbol_occurrence(
                &mut occurrences,
                &mut occurrence_indices,
                SymbolOccurrence {
                    span,
                    hover: format_builtin_signature(signature),
                    definition: None,
                },
            );
        }
    }

    occurrences.sort_by_key(|occurrence| (occurrence.span.start, occurrence.span.end));

    (!cancellation.is_cancelled()).then_some(ScriptSymbolIndex { occurrences })
}

/// Resolve an AST/prepared span to a span in the open root source. Without a
/// prepared map (the Legacy-only path), AST offsets already address the root.
fn map_index_span(
    prepared: Option<&crate::preprocessor::PreparedSource>,
    root_source: Option<crate::source::SourceId>,
    span: Span,
) -> Option<Span> {
    let (Some(prepared), Some(root_source)) = (prepared, root_source) else {
        return Some(span);
    };
    let prepared_span = prepared.prepared_span(span).ok()?;
    let resolved = prepared.map().resolve_span(prepared_span).ok()?;
    let source_span = resolved.primary();
    (source_span.source() == root_source)
        .then(|| prepared.map().render_span(source_span).ok())
        .flatten()
}

fn push_symbol_occurrence(
    occurrences: &mut Vec<SymbolOccurrence>,
    occurrence_indices: &mut HashMap<(usize, usize), usize>,
    occurrence: SymbolOccurrence,
) {
    let key = (occurrence.span.start, occurrence.span.end);
    if let Some(index) = occurrence_indices.get(&key).copied() {
        let existing = &mut occurrences[index];
        if existing.definition.is_none() && occurrence.definition.is_some() {
            *existing = occurrence;
        }
        return;
    }
    occurrence_indices.insert(key, occurrences.len());
    occurrences.push(occurrence);
}

fn handle_request(request: lsp_server::Request, state: &ServerState) -> Response {
    let lsp_server::Request { id, method, params } = request;
    if method != "textDocument/hover" && method != "textDocument/definition" {
        return Response {
            id,
            result: None,
            error: Some(ResponseError {
                code: lsp_server::ErrorCode::MethodNotFound as i32,
                message: format!("unsupported request: {method}"),
                data: None,
            }),
        };
    }
    let params = match serde_json::from_value::<TextDocumentPositionParams>(params) {
        Ok(params) => params,
        Err(error) => {
            return Response {
                id,
                result: None,
                error: Some(ResponseError {
                    code: lsp_server::ErrorCode::InvalidParams as i32,
                    message: format!("invalid text document position: {error}"),
                    data: None,
                }),
            };
        }
    };

    let result = if method == "textDocument/hover" {
        serde_json::to_value(hover_at(state, &params)).unwrap_or(Value::Null)
    } else {
        serde_json::to_value(definition_at(state, &params)).unwrap_or(Value::Null)
    };
    Response {
        id,
        result: Some(result),
        error: None,
    }
}

fn current_symbol_index<'a>(
    state: &'a ServerState,
    uri: &Url,
) -> Option<(&'a DocumentState, &'a ScriptSymbolIndex)> {
    let document = state.documents.get(uri)?;
    let cached = state.symbols_by_root.get(uri)?;
    (cached.version == document.version && cached.revision == document.revision)
        .then_some((document, cached.index.as_ref()))
}

fn hover_at(state: &ServerState, params: &TextDocumentPositionParams) -> Option<Hover> {
    let uri = &params.text_document.uri;
    let (document, index) = current_symbol_index(state, uri)?;
    let offset = position_to_offset(&document.text, params.position).ok()??;
    let occurrence = symbol_occurrence_at(index, offset)?;
    let range = span_to_range(&document.text, occurrence.span).ok()?;
    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: occurrence.hover.clone(),
        }),
        range: Some(range),
    })
}

fn definition_at(state: &ServerState, params: &TextDocumentPositionParams) -> Option<Location> {
    let uri = &params.text_document.uri;
    let (document, index) = current_symbol_index(state, uri)?;
    let offset = position_to_offset(&document.text, params.position).ok()??;
    let definition = symbol_occurrence_at(index, offset)?.definition?;
    Some(Location {
        uri: uri.clone(),
        range: span_to_range(&document.text, definition).ok()?,
    })
}

fn symbol_occurrence_at(index: &ScriptSymbolIndex, offset: usize) -> Option<&SymbolOccurrence> {
    index
        .occurrences
        .iter()
        .find(|occurrence| occurrence.span.start <= offset && offset < occurrence.span.end)
}

fn hover_for_user_symbol(function_signatures: &HashMap<usize, String>, symbol: &Symbol) -> String {
    let declaration = if symbol.kind == SymbolKind::Function {
        function_signatures
            .get(&symbol.span.start)
            .cloned()
            .unwrap_or_else(|| format!("function {}", symbol.name))
    } else {
        format!(
            "{} {}",
            symbol.type_name.as_deref().unwrap_or("value"),
            symbol.name
        )
    };
    format!("```lsl\n{declaration}\n```")
}

fn hover_for_builtin(scopes: &ScopeTree, name: &str) -> Option<String> {
    if let Some(signatures) = scopes.builtin_functions().get(name) {
        let rendered = signatures
            .iter()
            .map(format_builtin_signature)
            .collect::<Vec<_>>()
            .join("\n\n");
        if !rendered.is_empty() {
            return Some(rendered);
        }
    }
    if let Some(signature) = scopes.builtin_events().get(name) {
        return Some(format_builtin_signature(signature));
    }
    scopes.builtin_constants().get(name).map(|constant| {
        format!(
            "```lsl\n{} {} = {}\n```",
            constant.type_name, constant.name, constant.value
        )
    })
}

fn format_builtin_signature(signature: &Signature) -> String {
    let parameters = signature
        .parameters
        .iter()
        .map(|parameter| format!("{} {}", parameter.type_name, parameter.name))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "```lsl\n{} {}({})\n```",
        signature.return_type, signature.name, parameters
    )
}

/// Convert an LSP UTF-16 line/character pair into a UTF-8 byte offset.
///
/// A position inside a surrogate pair or beyond the source is not a valid
/// token location and returns `None`. CR, LF and CRLF mirror the forward
/// conversion used for diagnostics.
fn position_to_offset(text: &str, position: Position) -> Result<Option<usize>, String> {
    let mut line = 0u32;
    let mut character = 0u32;
    let mut previous_was_cr = false;
    for (offset, current) in text.char_indices() {
        if !(previous_was_cr && current == '\n')
            && line == position.line
            && character == position.character
        {
            return Ok(Some(offset));
        }
        match current {
            '\r' => {
                line = line.saturating_add(1);
                character = 0;
                previous_was_cr = true;
            }
            '\n' => {
                if !previous_was_cr {
                    line = line.saturating_add(1);
                    character = 0;
                }
                previous_was_cr = false;
            }
            _ => {
                if line == position.line {
                    let next = character.saturating_add(current.len_utf16() as u32);
                    if position.character < next {
                        return Ok(None);
                    }
                    character = next;
                }
                previous_was_cr = false;
            }
        }
        if line > position.line {
            return Ok(None);
        }
    }
    Ok((line == position.line && character == position.character).then_some(text.len()))
}

fn source_dependencies(
    root_uri: &Url,
    files: &[Arc<SourceFile>],
    include_dependencies: &HashSet<PathBuf>,
) -> HashSet<PathBuf> {
    let mut dependencies = HashSet::new();
    if let Ok(root_path) = root_uri.to_file_path() {
        dependencies.insert(normalize_path(&root_path));
        dependencies.insert(normalize_lexical_path(&root_path));
    }
    // The manager stores both canonical targets and post-unlink path
    // identities for symlink aliases. Do not canonicalize these again here.
    dependencies.extend(include_dependencies.iter().cloned());
    for source in files {
        if source.kind() != SourceFileKind::Included {
            continue;
        }
        let path = source
            .canonical_path()
            .cloned()
            .unwrap_or_else(|| PathBuf::from(source.display_name()));
        dependencies.insert(normalize_path(&path));
    }
    dependencies
}

fn uri_for_open_source(
    source: &SourceFile,
    open_paths: &HashMap<PathBuf, Url>,
    open_documents: &HashMap<Url, DocumentState>,
) -> Option<Url> {
    let path = source
        .canonical_path()
        .cloned()
        .or_else(|| Some(PathBuf::from(source.display_name())))?;
    let uri = open_paths.get(&normalize_path(&path))?.clone();
    let document = open_documents.get(&uri)?;
    (document.text.as_ref() == source.text()).then_some(uri)
}

fn normalize_path(path: &Path) -> PathBuf {
    normalize_local_path(path)
}

fn span_to_range(text: &str, span: Span) -> Result<Range, String> {
    let start = offset_to_position(text, span.start)?;
    let end = offset_to_position(text, span.end)?;
    Ok(Range { start, end })
}

fn source_span_to_range(text: &str, span: SourceSpan) -> Result<Range, String> {
    let start = offset_to_position(text, span.start())?;
    let end = offset_to_position(text, span.end())?;
    Ok(Range { start, end })
}

/// Convert a UTF-8 byte offset to an LSP UTF-16 line/character position.
///
/// NeoLint's internal spans are byte offsets. This conversion handles LF,
/// lone CR, CRLF and supplementary Unicode characters without relying on the
/// diagnostic's scalar-value column count.
fn offset_to_position(text: &str, offset: usize) -> Result<Position, String> {
    if offset > text.len() || !text.is_char_boundary(offset) {
        return Err(format!("invalid UTF-8 byte offset {offset}"));
    }
    let mut line = 0u32;
    let mut character = 0u32;
    let mut previous_was_cr = false;
    for current in text[..offset].chars() {
        match current {
            '\r' => {
                line = line.saturating_add(1);
                character = 0;
                previous_was_cr = true;
            }
            '\n' => {
                if !previous_was_cr {
                    line = line.saturating_add(1);
                    character = 0;
                }
                previous_was_cr = false;
            }
            _ => {
                character = character.saturating_add(current.len_utf16() as u32);
                previous_was_cr = false;
            }
        }
    }
    Ok(Position { line, character })
}

fn map_severity(severity: Severity) -> DiagnosticSeverity {
    match severity {
        Severity::Error => DiagnosticSeverity::ERROR,
        Severity::Warning => DiagnosticSeverity::WARNING,
        Severity::Info => DiagnosticSeverity::INFORMATION,
    }
}

fn publish_for_root(
    connection: &Connection,
    state: &mut ServerState,
    root_uri: &Url,
    mut diagnostics: RootDiagnostics,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    diagnostics.entry(root_uri.clone()).or_default();
    let affected_uris =
        replace_root_diagnostics(&mut state.published_by_root, root_uri.clone(), diagnostics);
    publish_aggregated_diagnostics(connection, state, affected_uris)
}

fn clear_root_diagnostics(
    connection: &Connection,
    state: &mut ServerState,
    root_uri: &Url,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut affected_uris = remove_root_diagnostics(&mut state.published_by_root, root_uri);
    if affected_uris.is_empty() {
        affected_uris.insert(root_uri.clone());
    }
    let mut affected_uris: Vec<_> = affected_uris.into_iter().collect();
    affected_uris.sort_by(|left, right| left.as_str().cmp(right.as_str()));

    for uri in affected_uris {
        // The document being closed must be cleared even if another open root
        // still has a cached include diagnostic for it. Other shared include
        // URIs are republished from the remaining roots below.
        let diagnostics = if &uri == root_uri {
            Vec::new()
        } else {
            aggregate_diagnostics(&uri, &state.published_by_root)
        };
        publish_diagnostics(connection, state, &uri, diagnostics)?;
    }
    Ok(())
}

/// Replace one root's diagnostic snapshot and return every URI whose aggregate
/// may have changed. Other roots' snapshots remain untouched.
fn replace_root_diagnostics(
    published_by_root: &mut HashMap<Url, RootDiagnostics>,
    root_uri: Url,
    diagnostics: RootDiagnostics,
) -> HashSet<Url> {
    let previous = published_by_root
        .insert(root_uri.clone(), diagnostics)
        .unwrap_or_default();
    let mut affected_uris: HashSet<_> = previous.into_keys().collect();
    if let Some(current) = published_by_root.get(&root_uri) {
        affected_uris.extend(current.keys().cloned());
    }
    affected_uris
}

/// Remove a root snapshot and return the URIs whose aggregate must be
/// republished from any roots that still reference them.
fn remove_root_diagnostics(
    published_by_root: &mut HashMap<Url, RootDiagnostics>,
    root_uri: &Url,
) -> HashSet<Url> {
    published_by_root
        .remove(root_uri)
        .map(|diagnostics| diagnostics.into_keys().collect())
        .unwrap_or_default()
}

/// Merge the diagnostic snapshots from all roots for one source URI.
/// Roots and affected URIs are sorted so publication order is deterministic.
/// Exact duplicates are collapsed when the same included finding is produced
/// by more than one root; distinct root-context findings are preserved.
fn aggregate_diagnostics(
    uri: &Url,
    published_by_root: &HashMap<Url, RootDiagnostics>,
) -> Vec<LspDiagnostic> {
    let mut roots: Vec<_> = published_by_root.iter().collect();
    roots.sort_by(|(left, _), (right, _)| left.as_str().cmp(right.as_str()));

    let mut aggregate = Vec::new();
    for (_, diagnostics_by_uri) in roots {
        if let Some(diagnostics) = diagnostics_by_uri.get(uri) {
            for diagnostic in diagnostics {
                if !aggregate.contains(diagnostic) {
                    aggregate.push(diagnostic.clone());
                }
            }
        }
    }
    aggregate
}

fn publish_aggregated_diagnostics(
    connection: &Connection,
    state: &ServerState,
    affected_uris: HashSet<Url>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut affected_uris: Vec<_> = affected_uris.into_iter().collect();
    affected_uris.sort_by(|left, right| left.as_str().cmp(right.as_str()));

    for uri in affected_uris {
        let diagnostics = aggregate_diagnostics(&uri, &state.published_by_root);
        publish_diagnostics(connection, state, &uri, diagnostics)?;
    }
    Ok(())
}

fn publish_diagnostics(
    connection: &Connection,
    state: &ServerState,
    uri: &Url,
    diagnostics: Vec<LspDiagnostic>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let version = state.documents.get(uri).map(|document| document.version);
    let params = PublishDiagnosticsParams {
        uri: uri.clone(),
        diagnostics,
        version,
    };
    connection.sender.send(Message::Notification(Notification {
        method: "textDocument/publishDiagnostics".to_owned(),
        params: serde_json::to_value(params)?,
    }))?;
    Ok(())
}

fn send_show_message(
    connection: &Connection,
    message_type: MessageType,
    message: &str,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    connection.sender.send(Message::Notification(Notification {
        method: "window/showMessage".to_owned(),
        params: json!({ "type": message_type, "message": message }),
    }))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn closing_an_untracked_document_does_not_remove_other_open_documents() {
        let tracked = Url::parse("file:///tmp/open.lsl").unwrap();
        let unrelated = Url::parse("file:///tmp/ignored.txt").unwrap();
        let mut documents = Arc::new(HashMap::from([(
            tracked.clone(),
            DocumentState {
                text: Arc::from("default {}"),
                version: 1,
                revision: 1,
            },
        )]));

        assert!(!remove_tracked_document(&mut documents, &unrelated));
        assert!(documents.contains_key(&tracked));
        assert!(remove_tracked_document(&mut documents, &tracked));
        assert!(documents.is_empty());
    }

    #[test]
    fn utf16_positions_handle_multibyte_and_surrogate_pairs() {
        let text = "aé🙂\r\nб";
        assert_eq!(offset_to_position(text, 0).unwrap(), Position::new(0, 0));
        assert_eq!(offset_to_position(text, 1).unwrap(), Position::new(0, 1));
        assert_eq!(offset_to_position(text, 3).unwrap(), Position::new(0, 2));
        assert_eq!(offset_to_position(text, 7).unwrap(), Position::new(0, 4));
        assert_eq!(offset_to_position(text, 9).unwrap(), Position::new(1, 0));
        assert_eq!(offset_to_position(text, 11).unwrap(), Position::new(1, 1));
    }

    #[test]
    fn utf16_positions_handle_lone_cr_and_lf() {
        let text = "first\rsecond\nthird";
        assert_eq!(offset_to_position(text, 6).unwrap(), Position::new(1, 0));
        assert_eq!(offset_to_position(text, 13).unwrap(), Position::new(2, 0));
    }

    #[test]
    fn utf16_positions_convert_back_to_utf8_offsets() {
        let text = "aé🙂\r\nб";
        assert_eq!(
            position_to_offset(text, Position::new(0, 0)).unwrap(),
            Some(0)
        );
        assert_eq!(
            position_to_offset(text, Position::new(0, 1)).unwrap(),
            Some(1)
        );
        assert_eq!(
            position_to_offset(text, Position::new(0, 2)).unwrap(),
            Some(3)
        );
        assert_eq!(
            position_to_offset(text, Position::new(0, 4)).unwrap(),
            Some(7)
        );
        assert_eq!(
            position_to_offset(text, Position::new(1, 0)).unwrap(),
            Some(9)
        );
        assert_eq!(
            position_to_offset(text, Position::new(1, 1)).unwrap(),
            Some(11)
        );
        // UTF-16 column 3 is between the two code units of the emoji.
        assert_eq!(position_to_offset(text, Position::new(0, 3)).unwrap(), None);
        assert_eq!(position_to_offset(text, Position::new(2, 0)).unwrap(), None);
        assert_eq!(
            position_to_offset("first\rsecond\nthird", Position::new(1, 0)).unwrap(),
            Some(6)
        );
        assert_eq!(
            position_to_offset("first\rsecond\nthird", Position::new(2, 0)).unwrap(),
            Some(13)
        );
    }

    #[test]
    fn rejects_offsets_outside_source_or_inside_utf8_scalar() {
        assert!(offset_to_position("é", 1).is_err());
        assert!(offset_to_position("é", 3).is_err());
    }

    #[test]
    fn ignores_stale_document_versions() {
        let uri = Url::parse("file:///tmp/hello.lsl").unwrap();
        let mut state = ServerState {
            config: Some(LintConfig::default()),
            documents: HashMap::from([(
                uri.clone(),
                DocumentState {
                    text: Arc::from("newer"),
                    version: 8,
                    revision: 1,
                },
            )])
            .into(),
            published_by_root: HashMap::new(),
            symbols_by_root: HashMap::new(),
            dependencies_by_root: HashMap::new(),
            dependency_tracking_incomplete_by_root: HashSet::new(),
            next_revision: 1,
            file_operation_revision: 0,
        };
        let stale_version = 7;
        let document = Arc::make_mut(&mut state.documents).get_mut(&uri).unwrap();
        if is_newer_version(document.version, stale_version) {
            document.text = Arc::from("older");
            document.version = stale_version;
        }
        assert_eq!(document.text.as_ref(), "newer");
        assert_eq!(document.version, 8);
        assert!(is_newer_version(8, 9));
        assert!(!is_newer_version(9, 9));
    }

    #[test]
    fn document_text_and_document_maps_are_shared_by_analysis_snapshots() {
        let uri = Url::parse("file:///tmp/shared.lsl").unwrap();
        let text: Arc<str> = Arc::from("default { state_entry() {} }");
        let documents = Arc::new(HashMap::from([(
            uri.clone(),
            DocumentState {
                text: Arc::clone(&text),
                version: 1,
                revision: 1,
            },
        )]));

        let first_snapshot = Arc::clone(&documents);
        let second_snapshot = Arc::clone(&documents);

        assert!(Arc::ptr_eq(&first_snapshot, &second_snapshot));
        assert!(Arc::ptr_eq(
            &first_snapshot.get(&uri).unwrap().text,
            &second_snapshot.get(&uri).unwrap().text
        ));

        // An edit gets a new immutable buffer and a new map snapshot. Existing
        // tasks keep the exact previous text they were scheduled with.
        let mut updated_documents = Arc::clone(&documents);
        let updated = Arc::make_mut(&mut updated_documents);
        updated.get_mut(&uri).unwrap().text = Arc::from("new contents");
        updated.get_mut(&uri).unwrap().revision = 2;

        assert_eq!(
            first_snapshot.get(&uri).unwrap().text.as_ref(),
            "default { state_entry() {} }"
        );
        assert_eq!(
            updated_documents.get(&uri).unwrap().text.as_ref(),
            "new contents"
        );
        assert!(!Arc::ptr_eq(&documents, &updated_documents));
    }

    #[test]
    fn pending_analysis_is_coalesced_per_root_without_starving_other_roots() {
        let uri_a = Url::parse("file:///tmp/a.lsl").unwrap();
        let uri_b = Url::parse("file:///tmp/b.lsl").unwrap();
        let make_task = |uri: &Url, version, revision| AnalysisTask {
            root_uri: uri.clone(),
            root_version: version,
            root_revision: revision,
            file_operation_revision: 0,
            documents: HashMap::from([(
                uri.clone(),
                DocumentState {
                    text: Arc::from(format!("version {version}")),
                    version,
                    revision,
                },
            )])
            .into(),
            config: LintConfig::default(),
        };
        let mut pending = PendingAnalyses::default();

        assert!(enqueue_pending(&mut pending, make_task(&uri_a, 1, 1)));
        assert!(enqueue_pending(&mut pending, make_task(&uri_b, 1, 2)));
        assert!(enqueue_pending(&mut pending, make_task(&uri_a, 2, 3)));
        assert_eq!(pending.tasks.len(), 2);
        assert_eq!(
            pending.order,
            VecDeque::from([uri_a.clone(), uri_b.clone()])
        );
        assert_eq!(pending.tasks.get(&uri_a).unwrap().root_version, 2);

        pending.stopped = true;
        assert!(!enqueue_pending(&mut pending, make_task(&uri_a, 3, 4)));
        assert_eq!(pending.tasks.get(&uri_a).unwrap().root_version, 2);
    }

    #[test]
    fn analysis_result_validates_only_root_and_actual_dependency_revisions() {
        let root_uri = Url::parse("file:///tmp/root.lsl").unwrap();
        let include_uri = Url::parse("file:///tmp/shared.inc").unwrap();
        let unrelated_uri = Url::parse("file:///tmp/unrelated.lsl").unwrap();
        let mut state = ServerState {
            config: Some(LintConfig::default()),
            documents: HashMap::from([
                (
                    root_uri.clone(),
                    DocumentState {
                        text: Arc::from("root"),
                        version: 4,
                        revision: 10,
                    },
                ),
                (
                    include_uri.clone(),
                    DocumentState {
                        text: Arc::from("include"),
                        version: 7,
                        revision: 11,
                    },
                ),
                (
                    unrelated_uri.clone(),
                    DocumentState {
                        text: Arc::from("unrelated"),
                        version: 2,
                        revision: 12,
                    },
                ),
            ])
            .into(),
            published_by_root: HashMap::new(),
            symbols_by_root: HashMap::new(),
            dependencies_by_root: HashMap::new(),
            dependency_tracking_incomplete_by_root: HashSet::new(),
            next_revision: 12,
            file_operation_revision: 0,
        };
        let result = AnalysisResult {
            root_uri: root_uri.clone(),
            root_version: 4,
            root_revision: 10,
            file_operation_revision: 0,
            dependencies: HashSet::from([
                normalize_path(&root_uri.to_file_path().unwrap()),
                normalize_path(&include_uri.to_file_path().unwrap()),
            ]),
            dependencies_complete: true,
            dependency_revisions: HashMap::from([
                (
                    normalize_path(&root_uri.to_file_path().unwrap()),
                    HashMap::from([(root_uri.clone(), 10)]),
                ),
                (
                    normalize_path(&include_uri.to_file_path().unwrap()),
                    HashMap::from([(include_uri.clone(), 11)]),
                ),
            ]),
            diagnostics: Ok(HashMap::new()),
            symbols: None,
        };

        assert!(analysis_snapshot_is_current(&state, &result));

        // An unrelated editor buffer may change or close without invalidating
        // a root that does not depend on it.
        Arc::make_mut(&mut state.documents)
            .get_mut(&unrelated_uri)
            .unwrap()
            .revision = 13;
        assert!(analysis_snapshot_is_current(&state, &result));
        Arc::make_mut(&mut state.documents).remove(&unrelated_uri);
        assert!(analysis_snapshot_is_current(&state, &result));

        // Editing or closing a dependency invalidates the root result.
        Arc::make_mut(&mut state.documents)
            .get_mut(&include_uri)
            .unwrap()
            .revision = 14;
        assert!(!analysis_snapshot_is_current(&state, &result));
        Arc::make_mut(&mut state.documents)
            .get_mut(&include_uri)
            .unwrap()
            .revision = 11;
        Arc::make_mut(&mut state.documents).remove(&include_uri);
        assert!(!analysis_snapshot_is_current(&state, &result));
        Arc::make_mut(&mut state.documents).insert(
            include_uri.clone(),
            DocumentState {
                text: Arc::from("include"),
                version: 7,
                revision: 11,
            },
        );

        // A dependency that was not open when the task started is captured as
        // `None`; opening it while analysis is running invalidates the result.
        let unopened_include = Url::parse("file:///tmp/new-include.inc").unwrap();
        let mut result_with_closed_dep = result;
        let unopened_path = normalize_path(&unopened_include.to_file_path().unwrap());
        result_with_closed_dep
            .dependencies
            .insert(unopened_path.clone());
        result_with_closed_dep
            .dependency_revisions
            .insert(unopened_path, HashMap::new());
        assert!(analysis_snapshot_is_current(
            &state,
            &result_with_closed_dep
        ));
        Arc::make_mut(&mut state.documents).insert(
            unopened_include,
            DocumentState {
                text: Arc::from("new include"),
                version: 1,
                revision: 15,
            },
        );
        assert!(!analysis_snapshot_is_current(
            &state,
            &result_with_closed_dep
        ));

        // Root closure always invalidates the result, independently of the
        // dependency map.
        Arc::make_mut(&mut state.documents).remove(&root_uri);
        assert!(!analysis_snapshot_is_current(
            &state,
            &result_with_closed_dep
        ));
    }

    #[test]
    fn workspace_file_operation_revision_rejects_pre_operation_results() {
        let root_uri = Url::parse("file:///tmp/file-operation-race.lsl").unwrap();
        let mut state = ServerState {
            config: Some(LintConfig::default()),
            documents: HashMap::from([(
                root_uri.clone(),
                DocumentState {
                    text: Arc::from("default { state_entry() {} }"),
                    version: 1,
                    revision: 1,
                },
            )])
            .into(),
            published_by_root: HashMap::new(),
            symbols_by_root: HashMap::new(),
            dependencies_by_root: HashMap::new(),
            dependency_tracking_incomplete_by_root: HashSet::new(),
            next_revision: 1,
            file_operation_revision: 7,
        };
        let result = AnalysisResult {
            root_uri,
            root_version: 1,
            root_revision: 1,
            file_operation_revision: 7,
            dependencies: HashSet::new(),
            dependencies_complete: true,
            dependency_revisions: HashMap::new(),
            diagnostics: Ok(HashMap::new()),
            symbols: None,
        };

        assert!(analysis_snapshot_is_current(&state, &result));
        // Document versions and dependency signatures stay exactly the same;
        // only the workspace filesystem generation changes.
        state.file_operation_revision += 1;
        assert!(
            !analysis_snapshot_is_current(&state, &result),
            "a result created before a file operation must never be published"
        );
    }

    #[test]
    fn pending_and_running_roots_are_visible_for_file_operation_requeue() {
        let pending_uri = Url::parse("file:///tmp/pending.lsl").unwrap();
        let running_uri = Url::parse("file:///tmp/running.lsl").unwrap();
        let make_task = |uri: &Url| AnalysisTask {
            root_uri: uri.clone(),
            root_version: 1,
            root_revision: 1,
            file_operation_revision: 3,
            documents: HashMap::from([(
                uri.clone(),
                DocumentState {
                    text: Arc::from("default { state_entry() {} }"),
                    version: 1,
                    revision: 1,
                },
            )])
            .into(),
            config: LintConfig::default(),
        };
        let pending = PendingAnalyses {
            tasks: HashMap::from([(pending_uri.clone(), make_task(&pending_uri))]),
            order: VecDeque::from([pending_uri.clone()]),
            running: Some(RunningAnalysis {
                root_uri: running_uri.clone(),
                cancellation: AnalysisCancellationToken::new(),
            }),
            stopped: false,
        };

        assert_eq!(
            pending_and_running_roots(&pending),
            HashSet::from([pending_uri, running_uri])
        );
    }

    #[test]
    fn incomplete_dependency_tracking_invalidates_on_any_local_document_change() {
        let root_uri = Url::parse("file:///tmp/incomplete-root.lsl").unwrap();
        let include_uri = Url::parse("file:///tmp/incomplete.inc").unwrap();
        let unrelated_uri = Url::parse("file:///tmp/unrelated.inc").unwrap();
        let mut state = ServerState {
            config: Some(LintConfig::default()),
            documents: HashMap::from([
                (
                    root_uri.clone(),
                    DocumentState {
                        text: Arc::from("root"),
                        version: 1,
                        revision: 1,
                    },
                ),
                (
                    include_uri.clone(),
                    DocumentState {
                        text: Arc::from("include"),
                        version: 1,
                        revision: 2,
                    },
                ),
                (
                    unrelated_uri.clone(),
                    DocumentState {
                        text: Arc::from("unrelated"),
                        version: 1,
                        revision: 3,
                    },
                ),
            ])
            .into(),
            published_by_root: HashMap::new(),
            symbols_by_root: HashMap::new(),
            dependencies_by_root: HashMap::from([(
                root_uri.clone(),
                HashSet::from([normalize_path(&root_uri.to_file_path().unwrap())]),
            )]),
            dependency_tracking_incomplete_by_root: HashSet::from([root_uri.clone()]),
            next_revision: 3,
            file_operation_revision: 0,
        };

        assert_eq!(
            affected_roots_for_document(&state, &unrelated_uri),
            vec![root_uri.clone()]
        );

        let dependencies = state.dependencies_by_root[&root_uri].clone();
        let snapshot = dependency_revision_snapshot(&dependencies, &state.documents, false);
        let result = AnalysisResult {
            root_uri: root_uri.clone(),
            root_version: 1,
            root_revision: 1,
            file_operation_revision: 0,
            dependencies,
            dependencies_complete: false,
            dependency_revisions: snapshot,
            diagnostics: Ok(HashMap::new()),
            symbols: None,
        };
        assert!(analysis_snapshot_is_current(&state, &result));

        Arc::make_mut(&mut state.documents)
            .get_mut(&unrelated_uri)
            .unwrap()
            .revision = 4;
        assert!(
            !analysis_snapshot_is_current(&state, &result),
            "an incomplete dependency index must guard the full open-document snapshot"
        );
    }

    #[cfg(unix)]
    #[test]
    fn dependency_snapshot_tracks_all_open_uri_aliases_for_a_local_path() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let workspace = std::env::temp_dir().join(format!("neolint-lsp-path-alias-{unique}"));
        fs::create_dir_all(&workspace).unwrap();
        let real_path = workspace.join("shared.inc");
        let alias_path = workspace.join("shared-alias.inc");
        fs::write(&real_path, "integer shared = 1;\n").unwrap();
        std::os::unix::fs::symlink(&real_path, &alias_path).unwrap();

        let root_uri = Url::from_file_path(workspace.join("root.lsl")).unwrap();
        let canonical_uri = Url::from_file_path(&real_path).unwrap();
        let alias_uri = Url::from_file_path(&alias_path).unwrap();
        let path = normalize_path(&real_path);
        let documents = HashMap::from([
            (
                root_uri.clone(),
                DocumentState {
                    text: Arc::from("root"),
                    version: 1,
                    revision: 1,
                },
            ),
            (
                canonical_uri.clone(),
                DocumentState {
                    text: Arc::from("include"),
                    version: 1,
                    revision: 2,
                },
            ),
        ]);
        let dependencies = HashSet::from([path.clone()]);
        let snapshot = dependency_revision_snapshot(&dependencies, &documents, true);
        assert_eq!(snapshot[&path], HashMap::from([(canonical_uri.clone(), 2)]));

        let mut with_alias = documents;
        with_alias.insert(
            alias_uri.clone(),
            DocumentState {
                text: Arc::from("include alias"),
                version: 1,
                revision: 3,
            },
        );
        let alias_snapshot = dependency_revision_snapshot(&dependencies, &with_alias, true);
        assert_eq!(
            alias_snapshot[&path],
            HashMap::from([(canonical_uri, 2), (alias_uri, 3)])
        );
        fs::remove_dir_all(workspace).unwrap();
    }

    #[test]
    fn affected_roots_are_only_roots_that_depend_on_the_changed_document() {
        let root_a = Url::parse("file:///tmp/a.lsl").unwrap();
        let root_b = Url::parse("file:///tmp/b.lsl").unwrap();
        let root_c = Url::parse("file:///tmp/c.lsl").unwrap();
        let shared = Url::parse("file:///tmp/shared.inc").unwrap();
        let private_a = Url::parse("file:///tmp/a-only.inc").unwrap();
        let documents = HashMap::from([
            (
                root_a.clone(),
                DocumentState {
                    text: Arc::from("a"),
                    version: 1,
                    revision: 1,
                },
            ),
            (
                root_b.clone(),
                DocumentState {
                    text: Arc::from("b"),
                    version: 1,
                    revision: 2,
                },
            ),
            (
                root_c.clone(),
                DocumentState {
                    text: Arc::from("c"),
                    version: 1,
                    revision: 3,
                },
            ),
        ]);
        let state = ServerState {
            config: Some(LintConfig::default()),
            documents: Arc::new(documents),
            published_by_root: HashMap::new(),
            symbols_by_root: HashMap::new(),
            dependencies_by_root: HashMap::from([
                (
                    root_a.clone(),
                    HashSet::from([
                        normalize_path(&root_a.to_file_path().unwrap()),
                        normalize_path(&shared.to_file_path().unwrap()),
                        normalize_path(&private_a.to_file_path().unwrap()),
                    ]),
                ),
                (
                    root_b.clone(),
                    HashSet::from([
                        normalize_path(&root_b.to_file_path().unwrap()),
                        normalize_path(&shared.to_file_path().unwrap()),
                    ]),
                ),
                (
                    root_c.clone(),
                    HashSet::from([normalize_path(&root_c.to_file_path().unwrap())]),
                ),
            ]),
            dependency_tracking_incomplete_by_root: HashSet::new(),
            next_revision: 3,
            file_operation_revision: 0,
        };

        assert_eq!(
            affected_roots_for_document(&state, &shared),
            vec![root_a.clone(), root_b.clone()]
        );
        assert_eq!(
            affected_roots_for_document(&state, &root_c),
            vec![root_c.clone()]
        );
        assert_eq!(affected_roots_for_document(&state, &root_a), vec![root_a]);
    }

    #[test]
    fn folder_operations_invalidate_include_dependencies_below_the_folder() {
        let root_uri = Url::parse("file:///tmp/project/main.lsl").unwrap();
        let folder_uri = Url::parse("file:///tmp/project/includes").unwrap();
        let nested_include = PathBuf::from("/tmp/project/includes/nested/shared.inc");
        let state = ServerState {
            config: Some(LintConfig::default()),
            documents: Arc::new(HashMap::from([(
                root_uri.clone(),
                DocumentState {
                    text: Arc::from("root"),
                    version: 1,
                    revision: 1,
                },
            )])),
            published_by_root: HashMap::new(),
            symbols_by_root: HashMap::new(),
            dependencies_by_root: HashMap::from([(
                root_uri.clone(),
                HashSet::from([
                    normalize_path(&root_uri.to_file_path().unwrap()),
                    normalize_path(&nested_include),
                ]),
            )]),
            dependency_tracking_incomplete_by_root: HashSet::new(),
            next_revision: 1,
            file_operation_revision: 0,
        };

        assert_eq!(
            affected_roots_for_document(&state, &folder_uri),
            vec![root_uri]
        );
    }

    #[test]
    fn folder_operations_match_lexical_paths_below_symlinked_directories() {
        let root_uri = Url::parse("file:///tmp/project/main.lsl").unwrap();
        let folder_uri = Url::parse("file:///tmp/project/include-alias").unwrap();
        let lexical_include = PathBuf::from("/tmp/project/include-alias/nested/shared.inc");
        let state = ServerState {
            config: Some(LintConfig::default()),
            documents: Arc::new(HashMap::from([(
                root_uri.clone(),
                DocumentState {
                    text: Arc::from("root"),
                    version: 1,
                    revision: 1,
                },
            )])),
            published_by_root: HashMap::new(),
            symbols_by_root: HashMap::new(),
            dependencies_by_root: HashMap::from([(
                root_uri.clone(),
                HashSet::from([
                    normalize_path(&root_uri.to_file_path().unwrap()),
                    normalize_lexical_path(&lexical_include),
                ]),
            )]),
            dependency_tracking_incomplete_by_root: HashSet::new(),
            next_revision: 1,
            file_operation_revision: 0,
        };

        assert_eq!(
            affected_roots_for_document(&state, &folder_uri),
            vec![root_uri]
        );
    }

    #[test]
    fn dependency_changes_cancel_only_an_active_affected_root() {
        let uri_a = Url::parse("file:///tmp/a.lsl").unwrap();
        let uri_b = Url::parse("file:///tmp/b.lsl").unwrap();
        let documents = Arc::new(HashMap::from([
            (
                uri_a.clone(),
                DocumentState {
                    text: Arc::from("a2"),
                    version: 2,
                    revision: 2,
                },
            ),
            (
                uri_b.clone(),
                DocumentState {
                    text: Arc::from("b2"),
                    version: 2,
                    revision: 3,
                },
            ),
        ]));
        let cancellation = AnalysisCancellationToken::new();
        let mut pending = PendingAnalyses {
            running: Some(RunningAnalysis {
                root_uri: uri_a.clone(),
                cancellation: cancellation.clone(),
            }),
            ..PendingAnalyses::default()
        };
        let make = |uri: &Url| {
            make_analysis_task(
                uri.clone(),
                Arc::clone(&documents),
                LintConfig::default(),
                0,
            )
            .unwrap()
        };

        // Editing an unrelated root does not interrupt the active analysis.
        assert!(schedule_pending_tasks(&mut pending, vec![make(&uri_b)]));
        assert!(!cancellation.is_cancelled());
        assert_eq!(pending.order, VecDeque::from([uri_b.clone()]));

        // A shared dependency change targets both roots and cancels the active
        // one; its replacement occupies the queued position exactly once.
        assert!(schedule_pending_tasks(
            &mut pending,
            vec![make(&uri_a), make(&uri_b)]
        ));
        assert!(cancellation.is_cancelled());
        assert_eq!(
            pending.order,
            VecDeque::from([uri_b.clone(), uri_a.clone()])
        );
        assert_eq!(pending.tasks.get(&uri_a).unwrap().root_version, 2);
    }

    fn test_diagnostic(message: &str) -> LspDiagnostic {
        LspDiagnostic {
            range: Range::new(Position::new(0, 0), Position::new(0, 1)),
            severity: Some(DiagnosticSeverity::WARNING),
            code: Some(lsp_types::NumberOrString::String("SEM999".to_owned())),
            source: Some(SERVER_NAME.to_owned()),
            message: message.to_owned(),
            ..Default::default()
        }
    }

    #[test]
    fn removing_one_root_preserves_diagnostics_shared_by_another_root() {
        let root_a = Url::parse("file:///workspace/a.lsl").unwrap();
        let root_b = Url::parse("file:///workspace/b.lsl").unwrap();
        let shared = Url::parse("file:///workspace/shared.inc").unwrap();
        let diagnostic_a = test_diagnostic("finding from root A");
        let diagnostic_b = test_diagnostic("finding from root B");
        let mut published_by_root = HashMap::from([
            (
                root_a.clone(),
                HashMap::from([(shared.clone(), vec![diagnostic_a.clone()])]),
            ),
            (
                root_b.clone(),
                HashMap::from([(shared.clone(), vec![diagnostic_b.clone()])]),
            ),
        ]);

        // Root A is reanalyzed and no longer includes the shared source.
        let affected =
            replace_root_diagnostics(&mut published_by_root, root_a.clone(), HashMap::new());
        assert!(affected.contains(&shared));
        assert_eq!(
            aggregate_diagnostics(&shared, &published_by_root),
            vec![diagnostic_b.clone()]
        );

        // Restore root A's old snapshot and test closing it independently.
        replace_root_diagnostics(
            &mut published_by_root,
            root_a.clone(),
            HashMap::from([(shared.clone(), vec![diagnostic_a])]),
        );
        let affected = remove_root_diagnostics(&mut published_by_root, &root_a);
        assert!(affected.contains(&shared));
        assert_eq!(
            aggregate_diagnostics(&shared, &published_by_root),
            vec![diagnostic_b]
        );

        // Only the last root removing the include clears the URI's diagnostics.
        let affected = remove_root_diagnostics(&mut published_by_root, &root_b);
        assert!(affected.contains(&shared));
        assert!(aggregate_diagnostics(&shared, &published_by_root).is_empty());
    }

    #[test]
    fn aggregation_is_deterministic_and_deduplicates_identical_findings() {
        let root_a = Url::parse("file:///workspace/a.lsl").unwrap();
        let root_b = Url::parse("file:///workspace/b.lsl").unwrap();
        let shared = Url::parse("file:///workspace/shared.inc").unwrap();
        let duplicate = test_diagnostic("same finding");
        let distinct = test_diagnostic("different context");
        let published_by_root = HashMap::from([
            (
                root_b,
                HashMap::from([(shared.clone(), vec![distinct.clone(), duplicate.clone()])]),
            ),
            (
                root_a,
                HashMap::from([(shared.clone(), vec![duplicate.clone()])]),
            ),
        ]);

        assert_eq!(
            aggregate_diagnostics(&shared, &published_by_root),
            vec![duplicate, distinct]
        );
    }

    #[test]
    fn source_dependencies_include_every_loaded_include_not_only_diagnostic_sources() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let workspace = std::env::temp_dir().join(format!("neolint-lsp-dependencies-{unique}"));
        fs::create_dir_all(&workspace).unwrap();
        let root_path = workspace.join("root.lsl");
        let include_path = workspace.join("shared.inc");
        fs::write(&include_path, "integer helper = 1;\n").unwrap();
        let root_uri = Url::from_file_path(&root_path).unwrap();
        let mut run = AnalysisRun::new().unwrap();
        let unit_index = run
            .add_preprocessed_text(
                root_path.to_str().unwrap(),
                "#include \"shared.inc\"\ndefault { state_entry() {} }\n",
                PreprocessSettings::full(),
            )
            .unwrap();
        let unit = run.unit(unit_index).unwrap();
        let dependencies = source_dependencies(
            &root_uri,
            unit.manager().files(),
            unit.manager().include_dependencies(),
        );
        assert!(dependencies.contains(&normalize_path(&root_uri.to_file_path().unwrap())));
        assert!(dependencies.contains(&normalize_path(&include_path)));
        fs::remove_dir_all(workspace).unwrap();
    }

    #[test]
    fn open_document_analysis_uses_the_configured_engine_and_publishes_root_diagnostics() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("neolint-lsp-analysis-{unique}"));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("script.lsl");
        let uri = Url::from_file_path(&path).unwrap();
        let text = "default { state_entry() { integer value = ; } }";
        let document = DocumentState {
            text: Arc::from(text),
            version: 4,
            revision: 1,
        };
        let open_documents = HashMap::from([(
            uri.clone(),
            DocumentState {
                text: Arc::from(text),
                version: 4,
                revision: 1,
            },
        )]);

        let actual = analyze_document(
            &uri,
            &document,
            &LintConfig::default(),
            &open_documents,
            &AnalysisCancellationToken::new(),
        )
        .unwrap();
        assert!(actual
            .dependencies
            .contains(&normalize_path(&uri.to_file_path().unwrap())));
        let actual_diagnostics = actual.diagnostics.get(&uri).unwrap();
        assert!(!actual_diagnostics.is_empty());
        assert!(actual_diagnostics
            .iter()
            .all(|diagnostic| diagnostic.source.as_deref() == Some(SERVER_NAME)));

        // The LSP adapter must expose the same rule codes and source ranges as
        // the configured public analysis API, not an independent lint path.
        let mut run = AnalysisRun::new().unwrap();
        let unit_index = run.add_prepared_text(path.to_str().unwrap(), text).unwrap();
        let expected = run
            .unit(unit_index)
            .unwrap()
            .analyze_with_config(&LintConfig::default(), &EngineAnalysisOptions::default())
            .unwrap();
        let expected_ranges: Vec<_> = expected
            .diagnostics()
            .iter()
            .map(|diagnostic| {
                (
                    diagnostic.diagnostic().code,
                    span_to_range(text, diagnostic.diagnostic().span).unwrap(),
                )
            })
            .collect();
        let actual_ranges: Vec<_> = actual_diagnostics
            .iter()
            .map(|diagnostic| {
                let code = match diagnostic.code.as_ref().unwrap() {
                    lsp_types::NumberOrString::String(code) => code.as_str(),
                    lsp_types::NumberOrString::Number(number) => {
                        panic!("unexpected numeric code {number}")
                    }
                };
                (code, diagnostic.range)
            })
            .collect();
        assert_eq!(actual_ranges, expected_ranges);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn workspace_root_is_the_last_include_fallback_for_preprocessed_documents() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("neolint-lsp-workspace-{unique}"));
        fs::create_dir_all(root.join("includes")).unwrap();
        fs::write(
            root.join(".neolint.conf"),
            "preprocess = true\ninclude_dir = includes\n",
        )
        .unwrap();
        let (config, error) = load_config(Some(&root), None);
        assert!(error.is_none(), "{error:?}");
        let config = config.unwrap();
        assert_eq!(
            config.include_dirs(),
            &[root.join("includes"), normalize_path(&root)]
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn config_include_paths_are_resolved_relative_to_the_config_file() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("neolint-lsp-config-{unique}"));
        fs::create_dir_all(root.join("includes")).unwrap();
        let config_path = root.join(".neolint.conf");
        fs::write(&config_path, "preprocess = true\ninclude_dir = includes\n").unwrap();
        let config = load_config_file(&config_path).unwrap();
        assert!(config.preprocess_enabled());
        assert_eq!(config.include_dirs(), &[root.join("includes")]);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn config_validation_matches_cli_include_and_target_requirements() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("neolint-lsp-validation-{unique}"));
        fs::create_dir_all(root.join("includes")).unwrap();
        fs::write(root.join("plain.conf"), "include_dir = includes\n").unwrap();
        let no_preprocess = load_config_file(&root.join("plain.conf")).unwrap_err();
        assert!(
            no_preprocess.contains("include directories require"),
            "{no_preprocess}"
        );

        fs::write(
            root.join("missing.conf"),
            "preprocess = true\ninclude_dir = missing\n",
        )
        .unwrap();
        let missing = load_config_file(&root.join("missing.conf")).unwrap_err();
        assert!(
            missing.contains("cannot access include directory"),
            "{missing}"
        );

        fs::write(
            root.join("legacy-target.conf"),
            "engine = legacy\ntarget = opensim-yengine\n",
        )
        .unwrap();
        let legacy_target = load_config_file(&root.join("legacy-target.conf")).unwrap_err();
        assert!(
            legacy_target.contains("target profiles require"),
            "{legacy_target}"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn lsl_document_filter_accepts_only_local_lsl_files() {
        assert!(is_lsl_document(
            &Url::parse("file:///tmp/script.lsl").unwrap()
        ));
        assert!(!is_lsl_document(
            &Url::parse("file:///tmp/script.luau").unwrap()
        ));
        assert!(!is_lsl_document(
            &Url::parse("untitled:script.lsl").unwrap()
        ));
    }
}
