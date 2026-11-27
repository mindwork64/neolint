use serde_json::{json, Value};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

fn frame(value: Value) -> String {
    let body = serde_json::to_string(&value).unwrap();
    format!("Content-Length: {}\r\n\r\n{}", body.len(), body)
}

fn read_frame(reader: &mut impl BufRead) -> std::io::Result<Option<Value>> {
    let mut content_length = None;
    let mut line = String::new();

    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
        if let Some(value) = line.strip_prefix("Content-Length:") {
            content_length = Some(
                value
                    .trim()
                    .parse::<usize>()
                    .map_err(std::io::Error::other)?,
            );
        }
    }

    let length = content_length.ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "missing Content-Length")
    })?;
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    serde_json::from_slice(&body)
        .map(Some)
        .map_err(std::io::Error::other)
}

struct LspClient {
    input: Option<ChildStdin>,
    messages: Receiver<Value>,
    child: Child,
    reader: Option<JoinHandle<()>>,
}

impl LspClient {
    fn new(workspace_uri: &str) -> Self {
        Self::with_file_operations(workspace_uri, true)
    }

    fn with_file_operations(workspace_uri: &str, supports_file_operations: bool) -> Self {
        let capabilities = if supports_file_operations {
            json!({
                "workspace": {
                    "fileOperations": {
                        "didCreate": true,
                        "didRename": true,
                        "didDelete": true
                    }
                }
            })
        } else {
            json!({})
        };
        let mut child = Command::new(env!("CARGO_BIN_EXE_neolint-lsp"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("start neolint-lsp binary");
        let input = child.stdin.take().expect("piped LSP stdin");
        let stdout = child.stdout.take().expect("piped LSP stdout");
        let (sender, messages) = mpsc::channel();

        let reader = thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                match read_frame(&mut reader) {
                    Ok(Some(message)) => {
                        if sender.send(message).is_err() {
                            return;
                        }
                    }
                    Ok(None) | Err(_) => return,
                }
            }
        });

        let mut client = Self {
            input: Some(input),
            messages,
            child,
            reader: Some(reader),
        };
        client.send(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "processId": null,
                "rootUri": workspace_uri,
                "capabilities": capabilities
            }
        }));
        let initialized = client.receive_until(|message| {
            message.get("id") == Some(&json!(1)) && message.get("result").is_some()
        });
        let initialize_result = initialized
            .iter()
            .find(|message| message.get("id") == Some(&json!(1)))
            .unwrap();
        if supports_file_operations {
            assert_eq!(
                initialize_result["result"]["workspace"]["fileOperations"]["didCreate"]
                    ["filters"][0]["scheme"],
                "file",
                "the server must advertise local file create notifications; response: {initialize_result:#?}"
            );
            assert!(
                initialize_result["result"]["workspace"]["fileOperations"]["didCreate"]["filters"]
                    [0]["pattern"]["matches"]
                    .is_null(),
                "file-operation filters must include files and folders"
            );
        } else {
            assert!(
                initialize_result["result"]["workspace"]["fileOperations"].is_null(),
                "the server must not advertise file operations the client did not request"
            );
        }
        client.send(json!({
            "jsonrpc": "2.0",
            "method": "initialized",
            "params": {}
        }));
        client
    }

    fn send(&mut self, message: Value) {
        self.input
            .as_mut()
            .expect("LSP client is still connected")
            .write_all(frame(message).as_bytes())
            .expect("write LSP frame");
    }

    fn receive_until(&self, mut predicate: impl FnMut(&Value) -> bool) -> Vec<Value> {
        let mut received = Vec::new();
        loop {
            let message = self
                .messages
                .recv_timeout(Duration::from_secs(15))
                .unwrap_or_else(|error| {
                    panic!("timed out waiting for LSP message ({error}); received: {received:#?}")
                });
            let matched = predicate(&message);
            received.push(message);
            if matched {
                return received;
            }
        }
    }

    fn assert_no_matching_message_for(
        &self,
        duration: Duration,
        mut predicate: impl FnMut(&Value) -> bool,
    ) {
        let deadline = Instant::now() + duration;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return;
            }
            match self.messages.recv_timeout(remaining) {
                Ok(message) => assert!(
                    !predicate(&message),
                    "received an unexpected LSP message: {message:#?}"
                ),
                Err(mpsc::RecvTimeoutError::Timeout) => return,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    panic!("LSP server disconnected while checking for unexpected messages")
                }
            }
        }
    }

    fn shutdown(mut self) {
        self.send(json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "shutdown",
            "params": null
        }));
        self.receive_until(|message| {
            message.get("id") == Some(&json!(2)) && message.get("result") == Some(&Value::Null)
        });
        self.send(json!({
            "jsonrpc": "2.0",
            "method": "exit",
            "params": null
        }));
        self.input.take();
        let status = self.child.wait().expect("wait for LSP server");
        assert!(status.success(), "LSP server exited with {status}");
        if let Some(reader) = self.reader.take() {
            reader.join().expect("join LSP stdout reader");
        }
    }
}

impl Drop for LspClient {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

fn is_publish(message: &Value, uri: &str, version: Option<i32>) -> bool {
    if message.get("method") != Some(&json!("textDocument/publishDiagnostics"))
        || message["params"]["uri"] != json!(uri)
    {
        return false;
    }
    match version {
        Some(version) => message["params"]["version"] == json!(version),
        None => message["params"]
            .get("version")
            .map_or(true, Value::is_null),
    }
}

fn unique_workspace(prefix: &str) -> std::path::PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let workspace = std::env::temp_dir().join(format!("{prefix}-{unique}"));
    fs::create_dir_all(&workspace).unwrap();
    workspace
}

#[test]
fn server_only_advertises_workspace_file_operations_when_client_supports_them() {
    let workspace = unique_workspace("neolint-lsp-file-op-negotiation");
    let workspace_uri = url::Url::from_directory_path(&workspace)
        .unwrap()
        .to_string();
    let client = LspClient::with_file_operations(&workspace_uri, false);
    client.shutdown();
    fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn stdio_lsp_publishes_current_versions_and_clears_diagnostics_on_close() {
    let workspace = unique_workspace("neolint-lsp-protocol");
    let source_path = workspace.join("script.lsl");
    let source_uri = url::Url::from_file_path(&source_path).unwrap().to_string();
    let workspace_uri = url::Url::from_directory_path(&workspace)
        .unwrap()
        .to_string();
    let mut client = LspClient::new(&workspace_uri);

    client.send(json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": source_uri,
                "languageId": "lsl",
                "version": 1,
                "text": "default { state_entry() { integer value = ; } }"
            }
        }
    }));
    let first = client.receive_until(|message| is_publish(message, &source_uri, Some(1)));
    let first_publication = first
        .iter()
        .find(|message| is_publish(message, &source_uri, Some(1)))
        .unwrap();
    assert!(!first_publication["params"]["diagnostics"]
        .as_array()
        .unwrap()
        .is_empty());

    client.send(json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didChange",
        "params": {
            "textDocument": { "uri": source_uri, "version": 2 },
            "contentChanges": [{
                "text": "default { state_entry() { llOwnerSay(\"fine\"); } }"
            }]
        }
    }));
    let second = client.receive_until(|message| is_publish(message, &source_uri, Some(2)));
    let second_publication = second
        .iter()
        .find(|message| is_publish(message, &source_uri, Some(2)))
        .unwrap();
    assert!(second_publication["params"]["diagnostics"]
        .as_array()
        .unwrap()
        .is_empty());

    // A delayed old snapshot must not replace the current document state.
    client.send(json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didChange",
        "params": {
            "textDocument": { "uri": source_uri, "version": 1 },
            "contentChanges": [{
                "text": "default { state_entry() { integer value = ; } }"
            }]
        }
    }));
    client.send(json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didClose",
        "params": { "textDocument": { "uri": source_uri } }
    }));
    let closed = client.receive_until(|message| is_publish(message, &source_uri, None));
    let cleared = closed
        .iter()
        .find(|message| is_publish(message, &source_uri, None))
        .unwrap();
    assert!(cleared["params"]["diagnostics"]
        .as_array()
        .unwrap()
        .is_empty());

    client.shutdown();
    fs::remove_dir_all(workspace).unwrap();
}

fn diagnostic_count_for_root(diagnostics: &[Value], root_suffix: &str) -> usize {
    diagnostics
        .iter()
        .filter(|diagnostic| {
            diagnostic["relatedInformation"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|related| {
                    related["location"]["uri"]
                        .as_str()
                        .is_some_and(|uri| uri.ends_with(root_suffix))
                })
        })
        .count()
}

fn diagnostic_roots(diagnostics: &[Value]) -> (bool, bool) {
    let mut root_a = false;
    let mut root_b = false;
    for diagnostic in diagnostics {
        for related in diagnostic["relatedInformation"]
            .as_array()
            .into_iter()
            .flatten()
        {
            let uri = related["location"]["uri"].as_str().unwrap_or_default();
            root_a |= uri.ends_with("/a.lsl");
            root_b |= uri.ends_with("/b.lsl");
        }
    }
    (root_a, root_b)
}

#[test]
fn shared_include_diagnostics_survive_reanalysis_of_one_root() {
    let workspace = unique_workspace("neolint-lsp-shared");
    let shared_path = workspace.join("shared.lsl");
    let root_a_path = workspace.join("a.lsl");
    let root_b_path = workspace.join("b.lsl");
    fs::write(
        &shared_path,
        "#ifdef ROOT_A\ninteger broken_a = ;\n#else\n#ifdef ROOT_B\ninteger broken_b = ;\n#else\ninteger fine = 1;\n#endif\n#endif\ndefault { state_entry() { llOwnerSay((string)fine); } }\n",
    )
    .unwrap();
    fs::write(&root_a_path, "#define ROOT_A\n#include \"shared.lsl\"\n").unwrap();
    fs::write(&root_b_path, "#define ROOT_B\n#include \"shared.lsl\"\n").unwrap();
    fs::write(workspace.join(".neolint.conf"), "preprocess = true\n").unwrap();

    let shared_uri = url::Url::from_file_path(&shared_path).unwrap().to_string();
    let root_a_uri = url::Url::from_file_path(&root_a_path).unwrap().to_string();
    let root_b_uri = url::Url::from_file_path(&root_b_path).unwrap().to_string();
    let workspace_uri = url::Url::from_directory_path(&workspace)
        .unwrap()
        .to_string();
    let shared_text = fs::read_to_string(&shared_path).unwrap();
    let root_a_text = fs::read_to_string(&root_a_path).unwrap();
    let root_b_text = fs::read_to_string(&root_b_path).unwrap();
    let valid_root_text = "default { state_entry() {} }";
    let mut client = LspClient::new(&workspace_uri);

    client.send(json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": shared_uri,
                "languageId": "lsl",
                "version": 1,
                "text": shared_text
            }
        }
    }));
    client.send(json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": root_a_uri,
                "languageId": "lsl",
                "version": 1,
                "text": root_a_text
            }
        }
    }));
    client.send(json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": root_b_uri,
                "languageId": "lsl",
                "version": 1,
                "text": root_b_text
            }
        }
    }));

    let initial = client.receive_until(|message| {
        if !is_publish(message, &shared_uri, Some(1)) {
            return false;
        }
        let diagnostics = message["params"]["diagnostics"].as_array().unwrap();
        diagnostic_roots(diagnostics) == (true, true)
    });
    let initial_shared = initial
        .iter()
        .rev()
        .find(|message| {
            if !is_publish(message, &shared_uri, Some(1)) {
                return false;
            }
            let diagnostics = message["params"]["diagnostics"].as_array().unwrap();
            diagnostic_roots(diagnostics) == (true, true)
        })
        .unwrap();
    assert!(!initial_shared["params"]["diagnostics"]
        .as_array()
        .unwrap()
        .is_empty());

    // Root A stops including shared.lsl. Root B's distinct diagnostic for the
    // same URI must remain published.
    client.send(json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didChange",
        "params": {
            "textDocument": { "uri": root_a_uri, "version": 2 },
            "contentChanges": [{ "text": valid_root_text }]
        }
    }));
    let after_a = client.receive_until(|message| {
        if !is_publish(message, &shared_uri, Some(1)) {
            return false;
        }
        let diagnostics = message["params"]["diagnostics"].as_array().unwrap();
        diagnostic_roots(diagnostics) == (false, true)
    });
    let after_a_shared = after_a
        .iter()
        .rev()
        .find(|message| {
            if !is_publish(message, &shared_uri, Some(1)) {
                return false;
            }
            let diagnostics = message["params"]["diagnostics"].as_array().unwrap();
            diagnostic_roots(diagnostics) == (false, true)
        })
        .unwrap();
    assert!(!after_a_shared["params"]["diagnostics"]
        .as_array()
        .unwrap()
        .is_empty());

    // When the final including root also stops referencing it, the shared URI
    // diagnostics are cleared.
    client.send(json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didChange",
        "params": {
            "textDocument": { "uri": root_b_uri, "version": 2 },
            "contentChanges": [{ "text": valid_root_text }]
        }
    }));
    let after_b = client.receive_until(|message| {
        is_publish(message, &shared_uri, Some(1))
            && message["params"]["diagnostics"]
                .as_array()
                .is_some_and(|diagnostics| diagnostics.is_empty())
    });
    assert!(after_b.iter().any(|message| {
        is_publish(message, &shared_uri, Some(1))
            && message["params"]["diagnostics"]
                .as_array()
                .is_some_and(|diagnostics| diagnostics.is_empty())
    }));

    client.shutdown();
    fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn unsaved_include_buffers_drive_preprocessing_and_diagnostics() {
    let workspace = unique_workspace("neolint-lsp-unsaved-includes");
    let shared_path = workspace.join("shared.inc");
    let root_path = workspace.join("main.lsl");
    let disk_text = "integer stable = 1;\n";
    let unsaved_invalid = "integer broken = ;\n";
    let unsaved_fixed = "integer stable = 2;\n";
    let root_text = concat!(
        "#include ",
        "\"shared.inc\"",
        "\n",
        "default { state_entry() { llOwnerSay((string)stable); } }\n",
    );
    fs::write(&shared_path, disk_text).unwrap();
    fs::write(&root_path, root_text).unwrap();
    fs::write(workspace.join(".neolint.conf"), "preprocess = true\n").unwrap();

    let shared_uri = url::Url::from_file_path(&shared_path).unwrap().to_string();
    let root_uri = url::Url::from_file_path(&root_path).unwrap().to_string();
    let workspace_uri = url::Url::from_directory_path(&workspace)
        .unwrap()
        .to_string();
    let mut client = LspClient::new(&workspace_uri);

    client.send(json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": shared_uri,
                "languageId": "lsl",
                "version": 1,
                "text": disk_text
            }
        }
    }));
    client.send(json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": root_uri,
                "languageId": "lsl",
                "version": 1,
                "text": root_text
            }
        }
    }));
    let initial = client.receive_until(|message| is_publish(message, &root_uri, Some(1)));
    let initial_publication = initial
        .iter()
        .find(|message| is_publish(message, &root_uri, Some(1)))
        .unwrap();
    assert!(initial_publication["params"]["diagnostics"]
        .as_array()
        .unwrap()
        .is_empty());

    // The client buffer changes but the include file on disk remains valid.
    client.send(json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didChange",
        "params": {
            "textDocument": { "uri": shared_uri, "version": 2 },
            "contentChanges": [{ "text": unsaved_invalid }]
        }
    }));
    let invalid = client.receive_until(|message| is_publish(message, &shared_uri, Some(2)));
    let invalid_publication = invalid
        .iter()
        .find(|message| is_publish(message, &shared_uri, Some(2)))
        .unwrap();
    assert!(
        !invalid_publication["params"]["diagnostics"]
            .as_array()
            .unwrap()
            .is_empty(),
        "unsaved include diagnostics must be based on the editor buffer"
    );
    assert_eq!(fs::read_to_string(&shared_path).unwrap(), disk_text);

    client.send(json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didChange",
        "params": {
            "textDocument": { "uri": shared_uri, "version": 3 },
            "contentChanges": [{ "text": unsaved_fixed }]
        }
    }));
    let fixed = client.receive_until(|message| is_publish(message, &shared_uri, Some(3)));
    let fixed_publication = fixed
        .iter()
        .find(|message| is_publish(message, &shared_uri, Some(3)))
        .unwrap();
    assert!(
        fixed_publication["params"]["diagnostics"]
            .as_array()
            .unwrap()
            .is_empty(),
        "fixing an unsaved include buffer must clear its diagnostics"
    );
    assert_eq!(fs::read_to_string(&shared_path).unwrap(), disk_text);

    client.shutdown();
    fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn oversized_unsaved_include_stays_tracked_until_the_buffer_is_fixed() {
    let workspace = unique_workspace("neolint-lsp-oversized-include");
    let shared_path = workspace.join("shared.inc");
    let root_path = workspace.join("main.lsl");
    let disk_text = "integer stable = 1;\n";
    let oversized_buffer = format!(
        "{}{}",
        disk_text,
        " ".repeat(neolint::source::MAX_INCLUDED_SOURCE_BYTES + 1 - disk_text.len())
    );
    let root_text = concat!(
        "#include ",
        "\"shared.inc\"",
        "\n",
        "default { state_entry() {} }\n",
    );
    fs::write(&shared_path, disk_text).unwrap();
    fs::write(&root_path, root_text).unwrap();
    fs::write(workspace.join(".neolint.conf"), "preprocess = true\n").unwrap();

    let shared_uri = url::Url::from_file_path(&shared_path).unwrap().to_string();
    let root_uri = url::Url::from_file_path(&root_path).unwrap().to_string();
    let workspace_uri = url::Url::from_directory_path(&workspace)
        .unwrap()
        .to_string();
    let mut client = LspClient::new(&workspace_uri);

    client.send(json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": shared_uri,
                "languageId": "lsl",
                "version": 1,
                "text": oversized_buffer
            }
        }
    }));
    client.send(json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": root_uri,
                "languageId": "lsl",
                "version": 1,
                "text": root_text
            }
        }
    }));
    let initial = client.receive_until(|message| is_publish(message, &root_uri, Some(1)));
    let initial_publication = initial
        .iter()
        .find(|message| is_publish(message, &root_uri, Some(1)))
        .unwrap();
    assert!(
        !initial_publication["params"]["diagnostics"]
            .as_array()
            .unwrap()
            .is_empty(),
        "an oversized unsaved include must produce a preprocessing diagnostic"
    );
    assert_eq!(fs::read_to_string(&shared_path).unwrap(), disk_text);

    // The same dependency must remain indexed even though its oversized buffer
    // was rejected before SourceManager registered it as an included source.
    client.send(json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didChange",
        "params": {
            "textDocument": { "uri": shared_uri, "version": 2 },
            "contentChanges": [{ "text": disk_text }]
        }
    }));
    let recovered = client.receive_until(|message| is_publish(message, &root_uri, Some(1)));
    let recovered_publication = recovered
        .iter()
        .find(|message| is_publish(message, &root_uri, Some(1)))
        .unwrap();
    assert!(
        recovered_publication["params"]["diagnostics"]
            .as_array()
            .unwrap()
            .is_empty(),
        "fixing the oversized include must reanalyze the dependent root"
    );
    assert_eq!(fs::read_to_string(&shared_path).unwrap(), disk_text);

    client.shutdown();
    fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn file_operation_during_initial_analysis_requeues_the_root() {
    let workspace = unique_workspace("neolint-lsp-file-operation-analysis-race");
    let include_path = workspace.join("shared.inc");
    let root_path = workspace.join("main.lsl");
    let root_text = concat!(
        "#include ",
        "\"shared.inc\"",
        "\n",
        "default { state_entry() { llOwnerSay((string)shared); } }\n",
    );
    fs::write(&root_path, root_text).unwrap();
    fs::write(workspace.join(".neolint.conf"), "preprocess = true\n").unwrap();

    let include_uri = url::Url::from_file_path(&include_path).unwrap().to_string();
    let root_uri = url::Url::from_file_path(&root_path).unwrap().to_string();
    let workspace_uri = url::Url::from_directory_path(&workspace)
        .unwrap()
        .to_string();
    let mut client = LspClient::new(&workspace_uri);

    // Do not wait for the first root publication. The file operation is sent
    // immediately after didOpen, so it can overlap either the dependency scan
    // or the worker-result handoff to the main LSP thread.
    client.send(json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": root_uri,
                "languageId": "lsl",
                "version": 1,
                "text": root_text
            }
        }
    }));
    fs::write(&include_path, "integer shared = 42;\n").unwrap();
    client.send(json!({
        "jsonrpc": "2.0",
        "method": "workspace/didCreateFiles",
        "params": { "files": [{ "uri": include_uri }] }
    }));

    let recovered = client.receive_until(|message| {
        is_publish(message, &root_uri, Some(1))
            && message["params"]["diagnostics"]
                .as_array()
                .is_some_and(|diagnostics| diagnostics.is_empty())
    });
    let publication = recovered
        .iter()
        .rev()
        .find(|message| {
            is_publish(message, &root_uri, Some(1))
                && message["params"]["diagnostics"]
                    .as_array()
                    .is_some_and(|diagnostics| diagnostics.is_empty())
        })
        .expect("the root must be reanalyzed against the created include");
    assert!(publication["params"]["diagnostics"]
        .as_array()
        .unwrap()
        .is_empty());

    client.shutdown();
    fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn workspace_file_operations_invalidate_missing_include_dependencies() {
    let workspace = unique_workspace("neolint-lsp-file-operations");
    let include_path = workspace.join("shared.inc");
    let renamed_path = workspace.join("renamed.inc");
    let root_path = workspace.join("main.lsl");
    let root_text = concat!(
        "#include ",
        "\"shared.inc\"",
        "\n",
        "default { state_entry() { llOwnerSay((string)shared); } }\n",
    );
    let include_text = "integer shared = 42;\n";
    fs::write(&root_path, root_text).unwrap();
    fs::write(workspace.join(".neolint.conf"), "preprocess = true\n").unwrap();

    let include_uri = url::Url::from_file_path(&include_path).unwrap().to_string();
    let renamed_uri = url::Url::from_file_path(&renamed_path).unwrap().to_string();
    let root_uri = url::Url::from_file_path(&root_path).unwrap().to_string();
    let workspace_uri = url::Url::from_directory_path(&workspace)
        .unwrap()
        .to_string();
    let mut client = LspClient::new(&workspace_uri);

    client.send(json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": root_uri,
                "languageId": "lsl",
                "version": 1,
                "text": root_text
            }
        }
    }));
    let missing = client.receive_until(|message| is_publish(message, &root_uri, Some(1)));
    let missing_publication = missing
        .iter()
        .find(|message| is_publish(message, &root_uri, Some(1)))
        .unwrap();
    assert!(!missing_publication["params"]["diagnostics"]
        .as_array()
        .unwrap()
        .is_empty());

    // The file is created on disk but never opened as a text document.
    fs::write(&include_path, include_text).unwrap();
    client.send(json!({
        "jsonrpc": "2.0",
        "method": "workspace/didCreateFiles",
        "params": { "files": [{ "uri": include_uri }] }
    }));
    let created = client.receive_until(|message| is_publish(message, &root_uri, Some(1)));
    let created_publication = created
        .iter()
        .find(|message| is_publish(message, &root_uri, Some(1)))
        .unwrap();
    assert!(
        created_publication["params"]["diagnostics"]
            .as_array()
            .unwrap()
            .is_empty(),
        "didCreateFiles must reanalyze roots watching the missing candidate"
    );

    // Renaming away the selected include restores the missing-file diagnostic.
    fs::rename(&include_path, &renamed_path).unwrap();
    client.send(json!({
        "jsonrpc": "2.0",
        "method": "workspace/didRenameFiles",
        "params": {
            "files": [{ "oldUri": include_uri, "newUri": renamed_uri }]
        }
    }));
    let renamed_away = client.receive_until(|message| is_publish(message, &root_uri, Some(1)));
    let renamed_away_publication = renamed_away
        .iter()
        .find(|message| is_publish(message, &root_uri, Some(1)))
        .unwrap();
    assert!(
        !renamed_away_publication["params"]["diagnostics"]
            .as_array()
            .unwrap()
            .is_empty(),
        "didRenameFiles must invalidate the old include path"
    );

    // Renaming a file back into the watched candidate path clears the error.
    fs::rename(&renamed_path, &include_path).unwrap();
    client.send(json!({
        "jsonrpc": "2.0",
        "method": "workspace/didRenameFiles",
        "params": {
            "files": [{ "oldUri": renamed_uri, "newUri": include_uri }]
        }
    }));
    let renamed_back = client.receive_until(|message| is_publish(message, &root_uri, Some(1)));
    let renamed_back_publication = renamed_back
        .iter()
        .find(|message| is_publish(message, &root_uri, Some(1)))
        .unwrap();
    assert!(
        renamed_back_publication["params"]["diagnostics"]
            .as_array()
            .unwrap()
            .is_empty(),
        "didRenameFiles must invalidate the new include candidate path"
    );

    fs::remove_file(&include_path).unwrap();
    client.send(json!({
        "jsonrpc": "2.0",
        "method": "workspace/didDeleteFiles",
        "params": { "files": [{ "uri": include_uri }] }
    }));
    let deleted = client.receive_until(|message| is_publish(message, &root_uri, Some(1)));
    let deleted_publication = deleted
        .iter()
        .find(|message| is_publish(message, &root_uri, Some(1)))
        .unwrap();
    assert!(
        !deleted_publication["params"]["diagnostics"]
            .as_array()
            .unwrap()
            .is_empty(),
        "didDeleteFiles must invalidate the removed include"
    );

    client.shutdown();
    fs::remove_dir_all(workspace).unwrap();
}

#[cfg(unix)]
#[test]
fn deleting_a_symlinked_include_reanalyzes_its_dependent_root() {
    let workspace = unique_workspace("neolint-lsp-symlink-include");
    let target_path = workspace.join("actual.inc");
    let alias_path = workspace.join("shared.inc");
    let root_path = workspace.join("main.lsl");
    let root_text = concat!(
        "#include ",
        "\"shared.inc\"",
        "\n",
        "default { state_entry() { llOwnerSay((string)shared); } }\n",
    );
    fs::write(&target_path, "integer shared = 1;\n").unwrap();
    std::os::unix::fs::symlink(&target_path, &alias_path).unwrap();
    fs::write(&root_path, root_text).unwrap();
    fs::write(workspace.join(".neolint.conf"), "preprocess = true\n").unwrap();

    let alias_uri = url::Url::from_file_path(&alias_path).unwrap().to_string();
    let root_uri = url::Url::from_file_path(&root_path).unwrap().to_string();
    let workspace_uri = url::Url::from_directory_path(&workspace)
        .unwrap()
        .to_string();
    let mut client = LspClient::new(&workspace_uri);

    client.send(json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": root_uri,
                "languageId": "lsl",
                "version": 1,
                "text": root_text
            }
        }
    }));
    let initial = client.receive_until(|message| is_publish(message, &root_uri, Some(1)));
    let initial_publication = initial
        .iter()
        .find(|message| is_publish(message, &root_uri, Some(1)))
        .unwrap();
    assert!(
        initial_publication["params"]["diagnostics"]
            .as_array()
            .unwrap()
            .is_empty(),
        "the symlinked include should initially resolve successfully"
    );

    // The target remains present; only the include alias is unlinked.
    fs::remove_file(&alias_path).unwrap();
    assert!(target_path.exists());
    client.send(json!({
        "jsonrpc": "2.0",
        "method": "workspace/didDeleteFiles",
        "params": { "files": [{ "uri": alias_uri }] }
    }));
    let deleted = client.receive_until(|message| is_publish(message, &root_uri, Some(1)));
    let deleted_publication = deleted
        .iter()
        .find(|message| is_publish(message, &root_uri, Some(1)))
        .unwrap();
    assert!(
        !deleted_publication["params"]["diagnostics"]
            .as_array()
            .unwrap()
            .is_empty(),
        "deleting a symlinked include must invalidate the root even while its target remains"
    );

    client.shutdown();
    fs::remove_dir_all(workspace).unwrap();
}

#[cfg(unix)]
#[test]
fn deleting_a_symlinked_include_directory_reanalyzes_its_dependent_root() {
    let workspace = unique_workspace("neolint-lsp-symlink-directory");
    let target_directory = workspace.join("actual-includes");
    let alias_directory = workspace.join("include-alias");
    let include_path = target_directory.join("shared.inc");
    let root_path = workspace.join("main.lsl");
    fs::create_dir_all(&target_directory).unwrap();
    fs::write(&include_path, "integer shared = 1;\n").unwrap();
    std::os::unix::fs::symlink(&target_directory, &alias_directory).unwrap();
    let root_text = concat!(
        "#include ",
        "\"include-alias/shared.inc\"",
        "\n",
        "default { state_entry() { llOwnerSay((string)shared); } }\n",
    );
    fs::write(&root_path, root_text).unwrap();
    fs::write(workspace.join(".neolint.conf"), "preprocess = true\n").unwrap();

    let alias_uri = url::Url::from_directory_path(&alias_directory)
        .unwrap()
        .to_string();
    let root_uri = url::Url::from_file_path(&root_path).unwrap().to_string();
    let workspace_uri = url::Url::from_directory_path(&workspace)
        .unwrap()
        .to_string();
    let mut client = LspClient::new(&workspace_uri);

    client.send(json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": root_uri,
                "languageId": "lsl",
                "version": 1,
                "text": root_text
            }
        }
    }));
    let initial = client.receive_until(|message| is_publish(message, &root_uri, Some(1)));
    let initial_publication = initial
        .iter()
        .find(|message| is_publish(message, &root_uri, Some(1)))
        .unwrap();
    assert!(
        initial_publication["params"]["diagnostics"]
            .as_array()
            .unwrap()
            .is_empty(),
        "include through a directory symlink should initially resolve"
    );

    // Remove only the alias directory symlink; the real include remains.
    fs::remove_file(&alias_directory).unwrap();
    assert!(include_path.exists());
    client.send(json!({
        "jsonrpc": "2.0",
        "method": "workspace/didDeleteFiles",
        "params": { "files": [{ "uri": alias_uri }] }
    }));
    let deleted = client.receive_until(|message| is_publish(message, &root_uri, Some(1)));
    let deleted_publication = deleted
        .iter()
        .find(|message| is_publish(message, &root_uri, Some(1)))
        .unwrap();
    assert!(
        !deleted_publication["params"]["diagnostics"]
            .as_array()
            .unwrap()
            .is_empty(),
        "deleting a symlinked include directory must invalidate nested includes"
    );

    client.shutdown();
    fs::remove_dir_all(workspace).unwrap();
}

#[cfg(unix)]
#[test]
fn deleting_symlinked_root_directory_reanalyzes_open_root() {
    let workspace = unique_workspace("neolint-lsp-symlink-root-directory");
    let real_directory = workspace.join("actual-scripts");
    let alias_directory = workspace.join("script-alias");
    fs::create_dir_all(&real_directory).unwrap();
    std::os::unix::fs::symlink(&real_directory, &alias_directory).unwrap();

    let real_root_path = real_directory.join("main.lsl");
    let alias_root_path = alias_directory.join("main.lsl");
    let root_uri = url::Url::from_file_path(&alias_root_path)
        .unwrap()
        .to_string();
    let alias_uri = url::Url::from_directory_path(&alias_directory)
        .unwrap()
        .to_string();
    let workspace_uri = url::Url::from_directory_path(&workspace)
        .unwrap()
        .to_string();
    let root_text = "default { state_entry() {} }\n";
    fs::write(&real_root_path, root_text).unwrap();
    let mut client = LspClient::new(&workspace_uri);

    client.send(json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": root_uri,
                "languageId": "lsl",
                "version": 1,
                "text": root_text
            }
        }
    }));
    let initial = client.receive_until(|message| is_publish(message, &root_uri, Some(1)));
    let initial_publication = initial
        .iter()
        .find(|message| is_publish(message, &root_uri, Some(1)))
        .unwrap();
    assert!(initial_publication["params"]["diagnostics"]
        .as_array()
        .unwrap()
        .is_empty());

    // The root itself stays open and the real directory remains present;
    // only the symlink spelling used by its URI disappears.
    fs::remove_file(&alias_directory).unwrap();
    assert!(real_root_path.exists());
    client.send(json!({
        "jsonrpc": "2.0",
        "method": "workspace/didDeleteFiles",
        "params": { "files": [{ "uri": alias_uri }] }
    }));
    let reanalyzed = client.receive_until(|message| is_publish(message, &root_uri, Some(1)));
    let reanalyzed_publication = reanalyzed
        .iter()
        .find(|message| is_publish(message, &root_uri, Some(1)))
        .unwrap();
    assert!(
        reanalyzed_publication["params"]["diagnostics"]
            .as_array()
            .unwrap()
            .is_empty(),
        "deleting the root's symlinked parent must still reanalyze the open root"
    );

    client.shutdown();
    fs::remove_dir_all(workspace).unwrap();
}

#[cfg(unix)]
#[test]
fn creating_include_candidate_after_symlink_parent_component_invalidates_root() {
    let workspace = unique_workspace("neolint-lsp-symlink-parent-component");
    let target_directory = workspace.join("actual");
    let symlink_target = target_directory.join("nested");
    let alias = workspace.join("alias");
    let fallback_directory = workspace.join("fallback");
    fs::create_dir_all(&symlink_target).unwrap();
    fs::create_dir_all(fallback_directory.join("nested")).unwrap();
    std::os::unix::fs::symlink(&symlink_target, &alias).unwrap();
    std::os::unix::fs::symlink(
        fallback_directory.join("nested"),
        fallback_directory.join("alias"),
    )
    .unwrap();

    let high_priority_path = target_directory.join("shared.inc");
    let fallback_path = fallback_directory.join("shared.inc");
    let root_path = workspace.join("main.lsl");
    fs::write(&fallback_path, "#define FALLBACK_INCLUDE\n").unwrap();
    let root_text = concat!(
        "#include ",
        "\"alias/../shared.inc\"",
        "\n",
        "#ifdef HIGH_PRIORITY\n",
        "integer broken = ;\n",
        "#endif\n",
        "default { state_entry() {} }\n",
    );
    fs::write(&root_path, root_text).unwrap();
    fs::write(
        workspace.join(".neolint.conf"),
        "preprocess = true\ninclude_dir = fallback\n",
    )
    .unwrap();

    let high_priority_uri = url::Url::from_file_path(&high_priority_path)
        .unwrap()
        .to_string();
    let root_uri = url::Url::from_file_path(&root_path).unwrap().to_string();
    let workspace_uri = url::Url::from_directory_path(&workspace)
        .unwrap()
        .to_string();
    let mut client = LspClient::new(&workspace_uri);

    client.send(json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": root_uri,
                "languageId": "lsl",
                "version": 1,
                "text": root_text
            }
        }
    }));
    let initial = client.receive_until(|message| is_publish(message, &root_uri, Some(1)));
    let initial_publication = initial
        .iter()
        .find(|message| is_publish(message, &root_uri, Some(1)))
        .unwrap();
    assert!(
        initial_publication["params"]["diagnostics"]
            .as_array()
            .unwrap()
            .is_empty(),
        "the fallback include should initially be selected; diagnostics: {:#?}",
        initial_publication["params"]["diagnostics"]
    );
    assert!(!high_priority_path.exists());

    // The filesystem follows the symlink before applying the parent component.
    fs::write(&high_priority_path, "#define HIGH_PRIORITY\n").unwrap();
    client.send(json!({
        "jsonrpc": "2.0",
        "method": "workspace/didCreateFiles",
        "params": { "files": [{ "uri": high_priority_uri }] }
    }));
    let shadowed = client.receive_until(|message| is_publish(message, &root_uri, Some(1)));
    let shadowed_publication = shadowed
        .iter()
        .find(|message| is_publish(message, &root_uri, Some(1)))
        .unwrap();
    assert!(
        !shadowed_publication["params"]["diagnostics"]
            .as_array()
            .unwrap()
            .is_empty(),
        "creating the actual higher-priority candidate must invalidate the root"
    );

    client.shutdown();
    fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn creating_a_higher_priority_include_candidate_changes_resolution() {
    let workspace = unique_workspace("neolint-lsp-include-precedence");
    let include_dir = workspace.join("fallback");
    fs::create_dir_all(&include_dir).unwrap();
    let selected_path = include_dir.join("shared.inc");
    let higher_priority_path = workspace.join("shared.inc");
    let root_path = workspace.join("main.lsl");
    let root_text = concat!(
        "#include ",
        "\"shared.inc\"",
        "\n",
        "#ifdef HIGH_PRIORITY\n",
        "integer broken = ;\n",
        "#endif\n",
        "default { state_entry() {} }\n",
    );
    fs::write(&selected_path, "integer shared = 1;\n").unwrap();
    fs::write(&root_path, root_text).unwrap();
    fs::write(
        workspace.join(".neolint.conf"),
        "preprocess = true\ninclude_dir = fallback\n",
    )
    .unwrap();

    let new_include_uri = url::Url::from_file_path(&higher_priority_path)
        .unwrap()
        .to_string();
    let root_uri = url::Url::from_file_path(&root_path).unwrap().to_string();
    let workspace_uri = url::Url::from_directory_path(&workspace)
        .unwrap()
        .to_string();
    let mut client = LspClient::new(&workspace_uri);

    client.send(json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": root_uri,
                "languageId": "lsl",
                "version": 1,
                "text": root_text
            }
        }
    }));
    let fallback = client.receive_until(|message| is_publish(message, &root_uri, Some(1)));
    let fallback_publication = fallback
        .iter()
        .find(|message| is_publish(message, &root_uri, Some(1)))
        .unwrap();
    assert!(
        fallback_publication["params"]["diagnostics"]
            .as_array()
            .unwrap()
            .is_empty(),
        "the valid lower-priority include should initially be selected"
    );

    // Adding a file in the source directory shadows the include-dir fallback.
    fs::write(&higher_priority_path, "#define HIGH_PRIORITY\n").unwrap();
    client.send(json!({
        "jsonrpc": "2.0",
        "method": "workspace/didCreateFiles",
        "params": { "files": [{ "uri": new_include_uri }] }
    }));
    let shadowed = client.receive_until(|message| is_publish(message, &root_uri, Some(1)));
    let shadowed_publication = shadowed
        .iter()
        .find(|message| is_publish(message, &root_uri, Some(1)))
        .unwrap();
    assert!(
        !shadowed_publication["params"]["diagnostics"]
            .as_array()
            .unwrap()
            .is_empty(),
        "creating a higher-priority candidate must invalidate and change the selected include"
    );

    client.shutdown();
    fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn opening_a_previously_missing_include_reanalyzes_its_dependent_root() {
    let workspace = unique_workspace("neolint-lsp-missing-include");
    let include_dir = workspace.join("includes");
    fs::create_dir_all(&include_dir).unwrap();
    let include_path = include_dir.join("shared.inc");
    let root_path = workspace.join("main.lsl");
    let root_text = concat!(
        "#include ",
        "\"includes/shared.inc\"",
        "\n",
        "default { state_entry() {} }\n",
    );
    fs::write(&root_path, root_text).unwrap();
    fs::write(workspace.join(".neolint.conf"), "preprocess = true\n").unwrap();

    let include_uri = url::Url::from_file_path(&include_path).unwrap().to_string();
    let root_uri = url::Url::from_file_path(&root_path).unwrap().to_string();
    let workspace_uri = url::Url::from_directory_path(&workspace)
        .unwrap()
        .to_string();
    let mut client = LspClient::new(&workspace_uri);

    client.send(json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": root_uri,
                "languageId": "lsl",
                "version": 1,
                "text": root_text
            }
        }
    }));
    let missing = client.receive_until(|message| is_publish(message, &root_uri, Some(1)));
    let missing_publication = missing
        .iter()
        .find(|message| is_publish(message, &root_uri, Some(1)))
        .unwrap();
    assert!(
        !missing_publication["params"]["diagnostics"]
            .as_array()
            .unwrap()
            .is_empty(),
        "a missing include must initially be diagnosed"
    );
    assert!(!include_path.exists());

    // The file appears after analysis and is opened with unsaved editor text.
    let include_text = "integer shared = 42;\n";
    fs::write(&include_path, "integer shared = 1;\n").unwrap();
    client.send(json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": include_uri,
                "languageId": "lsl",
                "version": 1,
                "text": include_text
            }
        }
    }));
    let recovered = client.receive_until(|message| is_publish(message, &root_uri, Some(1)));
    let recovered_publication = recovered
        .iter()
        .find(|message| is_publish(message, &root_uri, Some(1)))
        .unwrap();
    assert!(
        recovered_publication["params"]["diagnostics"]
            .as_array()
            .unwrap()
            .is_empty(),
        "opening a newly available include must reanalyze and clear the root error"
    );
    assert_eq!(
        fs::read_to_string(&include_path).unwrap(),
        "integer shared = 1;\n"
    );

    client.shutdown();
    fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn changing_a_shared_include_reanalyzes_only_dependent_roots() {
    let workspace = unique_workspace("neolint-lsp-dependencies");
    let shared_path = workspace.join("shared.inc");
    let root_a_path = workspace.join("a.lsl");
    let root_b_path = workspace.join("b.lsl");
    let root_c_path = workspace.join("unrelated.lsl");
    let shared_v1 = concat!(
        "#ifdef ROOT_A\n",
        "integer broken_a = ;\n",
        "#else\n",
        "#ifdef ROOT_B\n",
        "integer broken_b = ;\n",
        "#else\n",
        "integer fine = 1;\n",
        "#endif\n",
        "#endif\n",
        "default { state_entry() { llOwnerSay((string)fine); } }\n",
    );
    let shared_v2 = concat!(
        "#ifdef ROOT_A\n",
        "integer broken_a = ;\n",
        "integer added_a = ;\n",
        "#else\n",
        "#ifdef ROOT_B\n",
        "integer broken_b = ;\n",
        "integer added_b = ;\n",
        "#else\n",
        "integer fine = 1;\n",
        "#endif\n",
        "#endif\n",
        "default { state_entry() { llOwnerSay((string)fine); } }\n",
    );
    fs::write(&shared_path, shared_v1).unwrap();
    fs::write(&root_a_path, "#define ROOT_A\n#include \"shared.inc\"\n").unwrap();
    fs::write(&root_b_path, "#define ROOT_B\n#include \"shared.inc\"\n").unwrap();
    fs::write(
        &root_c_path,
        "default { state_entry() { integer unrelated = ; } }\n",
    )
    .unwrap();
    fs::write(workspace.join(".neolint.conf"), "preprocess = true\n").unwrap();

    let shared_uri = url::Url::from_file_path(&shared_path).unwrap().to_string();
    let root_a_uri = url::Url::from_file_path(&root_a_path).unwrap().to_string();
    let root_b_uri = url::Url::from_file_path(&root_b_path).unwrap().to_string();
    let root_c_uri = url::Url::from_file_path(&root_c_path).unwrap().to_string();
    let workspace_uri = url::Url::from_directory_path(&workspace)
        .unwrap()
        .to_string();
    let mut client = LspClient::new(&workspace_uri);

    for (uri, text) in [
        (shared_uri.as_str(), shared_v1),
        (
            root_a_uri.as_str(),
            "#define ROOT_A\n#include \"shared.inc\"\n",
        ),
        (
            root_b_uri.as_str(),
            "#define ROOT_B\n#include \"shared.inc\"\n",
        ),
        (
            root_c_uri.as_str(),
            "default { state_entry() { integer unrelated = ; } }\n",
        ),
    ] {
        client.send(json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": uri,
                    "languageId": "lsl",
                    "version": 1,
                    "text": text
                }
            }
        }));
    }

    let initial_shared = client.receive_until(|message| {
        if !is_publish(message, &shared_uri, Some(1)) {
            return false;
        }
        let diagnostics = message["params"]["diagnostics"].as_array().unwrap();
        diagnostic_roots(diagnostics) == (true, true)
    });
    let initial_publication = initial_shared
        .iter()
        .rev()
        .find(|message| {
            if !is_publish(message, &shared_uri, Some(1)) {
                return false;
            }
            diagnostic_roots(message["params"]["diagnostics"].as_array().unwrap()) == (true, true)
        })
        .unwrap();
    let initial_diagnostics = initial_publication["params"]["diagnostics"]
        .as_array()
        .unwrap()
        .len();
    assert!(initial_diagnostics > 0);

    // Wait until the unrelated root has published its own findings before the
    // shared include changes, so any later publication proves reanalysis.
    client.receive_until(|message| {
        is_publish(message, &root_c_uri, Some(1))
            && message["params"]["diagnostics"]
                .as_array()
                .is_some_and(|diagnostics| !diagnostics.is_empty())
    });

    // The preprocessor reads includes from disk. Keep the open buffer in sync
    // with disk for this test; unsaved include overlays are a separate feature.
    fs::write(&shared_path, shared_v2).unwrap();
    client.send(json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didChange",
        "params": {
            "textDocument": { "uri": shared_uri, "version": 2 },
            "contentChanges": [{ "text": shared_v2 }]
        }
    }));

    let after_change = client.receive_until(|message| {
        if !is_publish(message, &shared_uri, Some(2)) {
            return false;
        }
        let Some(diagnostics) = message["params"]["diagnostics"].as_array() else {
            return false;
        };
        diagnostic_roots(diagnostics) == (true, true)
            && diagnostics.len() > initial_diagnostics
            && diagnostic_count_for_root(diagnostics, "/a.lsl") > 0
            && diagnostic_count_for_root(diagnostics, "/b.lsl") > 0
    });
    assert!(!after_change
        .iter()
        .any(|message| is_publish(message, &root_c_uri, Some(1))));

    // The unrelated root is not in the dependency index for shared.inc and
    // must not receive a second diagnostics publication.
    client.assert_no_matching_message_for(Duration::from_millis(300), |message| {
        is_publish(message, &root_c_uri, Some(1))
    });

    // Closing the shared include invalidates every root that depends on it.
    // Once both parents have been reanalyzed, diagnostics for the closed URI
    // must be withdrawn even though the include still exists on disk.
    client.send(json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didClose",
        "params": { "textDocument": { "uri": shared_uri } }
    }));
    client.receive_until(|message| is_publish(message, &root_a_uri, Some(1)));
    client.receive_until(|message| is_publish(message, &root_b_uri, Some(1)));
    client.receive_until(|message| {
        is_publish(message, &shared_uri, None)
            && message["params"]["diagnostics"]
                .as_array()
                .is_some_and(|diagnostics| diagnostics.is_empty())
    });
    client.assert_no_matching_message_for(Duration::from_millis(300), |message| {
        is_publish(message, &root_c_uri, Some(1))
    });

    client.shutdown();
    fs::remove_dir_all(workspace).unwrap();
}
