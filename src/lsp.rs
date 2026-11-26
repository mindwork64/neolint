//! Language Server Protocol adapter for the existing NeoLint analysis API.
//!
//! The server deliberately owns no parser or semantic rules. Each open LSL
//! document is prepared through [`AnalysisRun`] and analyzed through
//! [`crate::source::AnalysisUnit::analyze_with_config`], the same configured entry point used
//! by library consumers.

use crate::config::LintConfig;
use crate::diagnostic::{Severity, Span};
use crate::engine::{AnalysisEngine, EngineAnalysisOptions};
use crate::pipeline::AnalysisRun;
use crate::preprocessor::PreprocessSettings;
use crate::source::{SourceFile, SourceSpan};
use lsp_server::{Connection, Message, Notification, Response, ResponseError};
use lsp_types::{
    Diagnostic as LspDiagnostic, DiagnosticRelatedInformation, DiagnosticSeverity,
    InitializeParams, Location, MessageType, Position, PublishDiagnosticsParams, Range,
    ServerCapabilities, TextDocumentSyncCapability, TextDocumentSyncKind, TextDocumentSyncOptions,
    Url,
};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

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
        documents: HashMap::new(),
        published_by_root: HashMap::new(),
    };

    for message in &connection.receiver {
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
                handle_notification(&connection, &mut state, notification)?;
            }
            Message::Response(_) => {}
        }
    }

    // `lsp-server`'s writer thread exits when the connection sender is
    // dropped. Drop the connection before joining the stdio threads.
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

#[derive(Debug)]
struct DocumentState {
    text: String,
    version: i32,
}

type RootDiagnostics = HashMap<Url, Vec<LspDiagnostic>>;

struct ServerState {
    config: Option<LintConfig>,
    documents: HashMap<Url, DocumentState>,
    /// Per-root diagnostic snapshots, keyed by the URI they belong to.
    /// A single included URI may be present in several root snapshots.
    published_by_root: HashMap<Url, RootDiagnostics>,
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
            state.documents.insert(
                document.uri.clone(),
                DocumentState {
                    text: document.text,
                    version: document.version,
                },
            );
            analyze_and_publish(connection, state, &document.uri)?;
        }
        "textDocument/didChange" => {
            let params: lsp_types::DidChangeTextDocumentParams =
                serde_json::from_value(notification.params)?;
            let uri = params.text_document.uri;
            let Some(change) = params.content_changes.into_iter().last() else {
                return Ok(());
            };
            if let Some(document) = state.documents.get_mut(&uri) {
                // LSP document versions are monotonic. Ignore delayed changes
                // from an older client snapshot instead of publishing them.
                if !is_newer_version(document.version, params.text_document.version) {
                    return Ok(());
                }
                document.text = change.text;
                document.version = params.text_document.version;
                analyze_and_publish(connection, state, &uri)?;
            }
        }
        "textDocument/didSave" => {
            let params: lsp_types::DidSaveTextDocumentParams =
                serde_json::from_value(notification.params)?;
            if let Some(document) = state.documents.get_mut(&params.text_document.uri) {
                if let Some(text) = params.text {
                    document.text = text;
                }
                analyze_and_publish(connection, state, &params.text_document.uri)?;
            }
        }
        "textDocument/didClose" => {
            let params: lsp_types::DidCloseTextDocumentParams =
                serde_json::from_value(notification.params)?;
            clear_root_diagnostics(connection, state, &params.text_document.uri)?;
            state.documents.remove(&params.text_document.uri);
        }
        _ => {}
    }
    Ok(())
}

fn is_lsl_document(uri: &Url) -> bool {
    uri.scheme() == "file" && uri.path().to_ascii_lowercase().ends_with(".lsl")
}

fn analyze_and_publish(
    connection: &Connection,
    state: &mut ServerState,
    root_uri: &Url,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let Some(document) = state.documents.get(root_uri) else {
        return Ok(());
    };
    let Some(config) = &state.config else {
        publish_for_root(connection, state, root_uri, HashMap::new())?;
        return Ok(());
    };

    match analyze_document(root_uri, document, config, &state.documents) {
        Ok(diagnostics) => publish_for_root(connection, state, root_uri, diagnostics)?,
        Err(error) => {
            eprintln!("NeoLint analysis failed for {root_uri}: {error}");
            publish_for_root(connection, state, root_uri, HashMap::new())?;
            send_show_message(
                connection,
                MessageType::WARNING,
                &format!("NeoLint could not analyze this document: {error}"),
            )?;
        }
    }
    Ok(())
}

fn analyze_document(
    root_uri: &Url,
    document: &DocumentState,
    config: &LintConfig,
    open_documents: &HashMap<Url, DocumentState>,
) -> Result<HashMap<Url, Vec<LspDiagnostic>>, String> {
    let root_path = root_uri
        .to_file_path()
        .map_err(|_| format!("unsupported document URI: {root_uri}"))?;
    let display_name = root_path.to_string_lossy().into_owned();

    let mut run = AnalysisRun::new().map_err(|error| error.to_string())?;
    let unit_index = if config.preprocess_enabled() {
        let settings = PreprocessSettings::full()
            .with_dialect(config.dialect())
            .with_include_dirs(config.include_dirs().to_vec());
        run.add_preprocessed_text(&display_name, &document.text, settings)
    } else if config.engine() == AnalysisEngine::Legacy {
        run.add_legacy_text(&display_name, &document.text)
    } else {
        run.add_prepared_text(&display_name, &document.text)
    }
    .map_err(|error| error.to_string())?;

    let unit = run
        .unit(unit_index)
        .ok_or_else(|| "analysis unit disappeared after construction".to_owned())?;
    let analysis = unit
        .analyze_with_config(config, &EngineAnalysisOptions::default())
        .map_err(|error| error.to_string())?;

    let mut open_paths = HashMap::<PathBuf, Url>::new();
    for uri in open_documents.keys() {
        if let Ok(path) = uri.to_file_path() {
            open_paths.insert(normalize_path(&path), uri.clone());
        }
    }

    let mut output = HashMap::<Url, Vec<LspDiagnostic>>::new();
    output.entry(root_uri.clone()).or_default();
    let root_source = unit.root();

    for rendered in analysis.diagnostics() {
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
        if source_file.text() != open_source.text {
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
            if related_file.text() != related_document.text {
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
    (document.text == source.text()).then_some(uri)
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
                    text: "newer".to_owned(),
                    version: 8,
                },
            )]),
            published_by_root: HashMap::new(),
        };
        let stale_version = 7;
        let document = state.documents.get_mut(&uri).unwrap();
        if is_newer_version(document.version, stale_version) {
            document.text = "older".to_owned();
            document.version = stale_version;
        }
        assert_eq!(document.text, "newer");
        assert_eq!(document.version, 8);
        assert!(is_newer_version(8, 9));
        assert!(!is_newer_version(9, 9));
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
            text: text.to_owned(),
            version: 4,
        };
        let open_documents = HashMap::from([(
            uri.clone(),
            DocumentState {
                text: text.to_owned(),
                version: 4,
            },
        )]);

        let actual =
            analyze_document(&uri, &document, &LintConfig::default(), &open_documents).unwrap();
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
