//! Language Server Protocol adapter for the existing NeoLint analysis API.
//!
//! The server deliberately owns no parser or semantic rules. Each open LSL
//! document is prepared through [`AnalysisRun`] and analyzed through
//! [`crate::source::AnalysisUnit::analyze_with_config`], the same configured entry point used
//! by library consumers.

use crate::cancellation::AnalysisCancellationToken;
use crate::config::LintConfig;
use crate::diagnostic::{Severity, Span};
use crate::engine::{AnalysisEngine, EngineAnalysisOptions};
use crate::pipeline::{AnalysisRun, CancellableSetupError};
use crate::preprocessor::PreprocessSettings;
use crate::source::unit::CancellableAnalysisError;
use crate::source::{SourceFile, SourceSpan};
use crossbeam_channel::{select, Receiver, Sender};
use lsp_server::{Connection, Message, Notification, Response, ResponseError};
use lsp_types::{
    Diagnostic as LspDiagnostic, DiagnosticRelatedInformation, DiagnosticSeverity,
    InitializeParams, Location, MessageType, Position, PublishDiagnosticsParams, Range,
    ServerCapabilities, TextDocumentSyncCapability, TextDocumentSyncKind, TextDocumentSyncOptions,
    Url,
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

    let capabilities = ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Options(
            TextDocumentSyncOptions {
                open_close: Some(true),
                change: Some(TextDocumentSyncKind::FULL),
                save: Some(lsp_types::TextDocumentSyncSaveOptions::Supported(true)),
                ..Default::default()
            },
        )),
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
        next_revision: 0,
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
                        let response = Response {
                            id: request.id,
                            result: None,
                            error: Some(ResponseError {
                                code: lsp_server::ErrorCode::MethodNotFound as i32,
                                message: format!("unsupported request: {}", request.method),
                                data: None,
                            }),
                        };
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
    next_revision: u64,
}

struct AnalysisTask {
    root_uri: Url,
    root_version: i32,
    documents: Arc<HashMap<Url, DocumentState>>,
    config: LintConfig,
}

struct AnalysisResult {
    root_uri: Url,
    root_version: i32,
    snapshot_revisions: HashMap<Url, u64>,
    diagnostics: Result<RootDiagnostics, String>,
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
/// running; its result is accepted only while every open-document snapshot
/// still matches the live document revisions.
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

    fn schedule(
        &self,
        root_uri: Url,
        documents: Arc<HashMap<Url, DocumentState>>,
        config: LintConfig,
    ) {
        let Some(task) =
            make_analysis_task(root_uri.clone(), Arc::clone(&documents), config.clone())
        else {
            return;
        };
        let (lock, ready) = &*self.pending;
        let mut pending = lock.lock().expect("analysis queue mutex poisoned");
        if schedule_pending_task(&mut pending, task, &documents, &config) {
            ready.notify_one();
        }
    }

    fn cancel_pending(
        &self,
        root_uri: &Url,
        documents: &Arc<HashMap<Url, DocumentState>>,
        config: Option<&LintConfig>,
    ) {
        let (lock, ready) = &*self.pending;
        let mut pending = lock.lock().expect("analysis queue mutex poisoned");
        pending.tasks.remove(root_uri);
        pending.order.retain(|uri| uri != root_uri);
        if let Some(running) = &pending.running {
            // Closing any document invalidates every in-flight snapshot because
            // tasks capture the complete open-document set.
            running.cancellation.cancel();
            if &running.root_uri != root_uri {
                if let Some(config) = config {
                    if let Some(retry) = make_analysis_task(
                        running.root_uri.clone(),
                        Arc::clone(documents),
                        config.clone(),
                    ) {
                        enqueue_pending(&mut pending, retry);
                    }
                }
            }
        }
        if let Some(config) = config {
            refresh_pending_tasks(&mut pending, documents, config);
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
) -> Option<AnalysisTask> {
    let root = documents.get(&root_uri)?;
    Some(AnalysisTask {
        root_uri,
        root_version: root.version,
        documents,
        config,
    })
}

fn schedule_pending_task(
    pending: &mut PendingAnalyses,
    task: AnalysisTask,
    documents: &Arc<HashMap<Url, DocumentState>>,
    config: &LintConfig,
) -> bool {
    if let Some(running) = &pending.running {
        // Every task captures the complete open-document set. Any edit makes
        // the active snapshot stale, even when a different root was edited.
        running.cancellation.cancel();
        if running.root_uri != task.root_uri {
            // Preserve the interrupted root in FIFO order using the newest
            // document snapshot, rather than losing its pending diagnostics.
            if let Some(retry) = make_analysis_task(
                running.root_uri.clone(),
                Arc::clone(documents),
                config.clone(),
            ) {
                enqueue_pending(pending, retry);
            }
        }
    }
    refresh_pending_tasks(pending, documents, config);
    enqueue_pending(pending, task)
}

fn refresh_pending_tasks(
    pending: &mut PendingAnalyses,
    documents: &Arc<HashMap<Url, DocumentState>>,
    config: &LintConfig,
) {
    let mut refreshed_tasks = HashMap::with_capacity(pending.tasks.len());
    let mut refreshed_order = VecDeque::with_capacity(pending.order.len());
    for uri in pending.order.drain(..) {
        if let Some(task) = make_analysis_task(uri.clone(), Arc::clone(documents), config.clone()) {
            refreshed_order.push_back(uri.clone());
            refreshed_tasks.insert(uri, task);
        }
    }
    pending.tasks = refreshed_tasks;
    pending.order = refreshed_order;
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

fn analysis_snapshot_is_current(state: &ServerState, result: &AnalysisResult) -> bool {
    state.documents.len() == result.snapshot_revisions.len()
        && result.snapshot_revisions.iter().all(|(uri, revision)| {
            state
                .documents
                .get(uri)
                .is_some_and(|document| document.revision == *revision)
        })
        && state
            .documents
            .get(&result.root_uri)
            .is_some_and(|document| document.version == result.root_version)
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

        let snapshot_revisions = task
            .documents
            .iter()
            .map(|(uri, document)| (uri.clone(), document.revision))
            .collect();
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
        let diagnostics = match result {
            Ok(diagnostics) => Ok(diagnostics),
            Err(AnalyzeDocumentError::Failure(error)) => Err(error),
            Err(AnalyzeDocumentError::Cancelled) => continue,
        };
        if result_sender
            .send(AnalysisResult {
                root_uri: task.root_uri,
                root_version: task.root_version,
                snapshot_revisions,
                diagnostics,
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
            if !is_lsl_document(&document.uri) {
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
            analyze_or_schedule(connection, state, worker, &document.uri)?;
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
            analyze_or_schedule(connection, state, worker, &uri)?;
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
                analyze_or_schedule(connection, state, worker, &uri)?;
            }
        }
        "textDocument/didClose" => {
            let params: lsp_types::DidCloseTextDocumentParams =
                serde_json::from_value(notification.params)?;
            let was_tracked =
                remove_tracked_document(&mut state.documents, &params.text_document.uri);
            if was_tracked {
                worker.cancel_pending(
                    &params.text_document.uri,
                    &state.documents,
                    state.config.as_ref(),
                );
            }
            clear_root_diagnostics(connection, state, &params.text_document.uri)?;
        }
        _ => {}
    }
    Ok(())
}

fn remove_tracked_document(documents: &mut Arc<HashMap<Url, DocumentState>>, uri: &Url) -> bool {
    Arc::make_mut(documents).remove(uri).is_some()
}

fn is_lsl_document(uri: &Url) -> bool {
    uri.scheme() == "file" && uri.path().to_ascii_lowercase().ends_with(".lsl")
}

fn analyze_or_schedule(
    connection: &Connection,
    state: &mut ServerState,
    worker: &AnalysisWorker,
    root_uri: &Url,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    if !state.documents.contains_key(root_uri) {
        return Ok(());
    }
    let Some(config) = &state.config else {
        publish_for_root(connection, state, root_uri, HashMap::new())?;
        return Ok(());
    };

    worker.schedule(
        root_uri.clone(),
        Arc::clone(&state.documents),
        config.clone(),
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
        analyze_or_schedule(connection, state, worker, &result.root_uri)?;
        return Ok(());
    }

    match result.diagnostics {
        Ok(diagnostics) => publish_for_root(connection, state, &result.root_uri, diagnostics)?,
        Err(error) => {
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
) -> Result<HashMap<Url, Vec<LspDiagnostic>>, AnalyzeDocumentError> {
    if cancellation.is_cancelled() {
        return Err(AnalyzeDocumentError::Cancelled);
    }
    let root_path = root_uri.to_file_path().map_err(|_| {
        AnalyzeDocumentError::Failure(format!("unsupported document URI: {root_uri}"))
    })?;
    let display_name = root_path.to_string_lossy().into_owned();

    let mut run =
        AnalysisRun::new().map_err(|error| AnalyzeDocumentError::Failure(error.to_string()))?;
    let unit_index = if config.preprocess_enabled() {
        let settings = PreprocessSettings::full()
            .with_dialect(config.dialect())
            .with_include_dirs(config.include_dirs().to_vec());
        run.add_preprocessed_text_cancellable(&display_name, &document.text, settings, cancellation)
            .map_err(|error| match error {
                CancellableSetupError::Cancelled => AnalyzeDocumentError::Cancelled,
                CancellableSetupError::Setup(error) => {
                    AnalyzeDocumentError::Failure(error.to_string())
                }
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

    let mut open_paths = HashMap::<PathBuf, Url>::new();
    for uri in open_documents.keys() {
        if cancellation.is_cancelled() {
            return Err(AnalyzeDocumentError::Cancelled);
        }
        if let Ok(path) = uri.to_file_path() {
            open_paths.insert(normalize_path(&path), uri.clone());
        }
    }

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
        // Included files are currently read from disk by the preprocessor.
        // Do not publish their diagnostics against a different, unsaved editor
        // buffer. Open-document overlays are a later workspace milestone.
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
    Ok(output)
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
    fs::canonicalize(path).unwrap_or_else(|_| {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(path)
        }
    })
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
            next_revision: 1,
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
    fn new_snapshot_cancels_active_analysis_and_requeues_it_before_the_new_root() {
        let uri_a = Url::parse("file:///tmp/running.lsl").unwrap();
        let uri_b = Url::parse("file:///tmp/edited.lsl").unwrap();
        let documents: Arc<HashMap<Url, DocumentState>> = Arc::new(HashMap::from([
            (
                uri_a.clone(),
                DocumentState {
                    text: Arc::from("root A v2"),
                    version: 2,
                    revision: 2,
                },
            ),
            (
                uri_b.clone(),
                DocumentState {
                    text: Arc::from("root B v3"),
                    version: 3,
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
        let task_b =
            make_analysis_task(uri_b.clone(), Arc::clone(&documents), LintConfig::default())
                .unwrap();

        assert!(schedule_pending_task(
            &mut pending,
            task_b,
            &documents,
            &LintConfig::default(),
        ));
        assert!(cancellation.is_cancelled());
        assert_eq!(
            pending.order,
            VecDeque::from([uri_a.clone(), uri_b.clone()])
        );
        assert_eq!(pending.tasks.get(&uri_a).unwrap().root_version, 2);
        assert_eq!(pending.tasks.get(&uri_b).unwrap().root_version, 3);
        assert!(Arc::ptr_eq(
            &pending.tasks.get(&uri_a).unwrap().documents,
            &documents
        ));
    }

    #[test]
    fn analysis_result_is_rejected_when_any_open_document_snapshot_changes() {
        let root_uri = Url::parse("file:///tmp/root.lsl").unwrap();
        let include_uri = Url::parse("file:///tmp/shared.lsl").unwrap();
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
            ])
            .into(),
            published_by_root: HashMap::new(),
            next_revision: 11,
        };
        let result = AnalysisResult {
            root_uri: root_uri.clone(),
            root_version: 4,
            snapshot_revisions: HashMap::from([(root_uri.clone(), 10), (include_uri.clone(), 11)]),
            diagnostics: Ok(HashMap::new()),
        };

        assert!(analysis_snapshot_is_current(&state, &result));

        Arc::make_mut(&mut state.documents)
            .get_mut(&include_uri)
            .unwrap()
            .revision = 12;
        assert!(!analysis_snapshot_is_current(&state, &result));

        Arc::make_mut(&mut state.documents)
            .get_mut(&include_uri)
            .unwrap()
            .revision = 11;
        Arc::make_mut(&mut state.documents).remove(&root_uri);
        assert!(!analysis_snapshot_is_current(&state, &result));

        Arc::make_mut(&mut state.documents).insert(
            root_uri.clone(),
            DocumentState {
                text: Arc::from("root"),
                version: 4,
                revision: 10,
            },
        );
        let newly_opened_uri = Url::parse("file:///tmp/new.lsl").unwrap();
        Arc::make_mut(&mut state.documents).insert(
            newly_opened_uri,
            DocumentState {
                text: Arc::from("new"),
                version: 1,
                revision: 12,
            },
        );
        assert!(!analysis_snapshot_is_current(&state, &result));
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
        let actual_diagnostics = actual.get(&uri).unwrap();
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
