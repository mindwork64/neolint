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
                "capabilities": {}
            }
        }));
        client.receive_until(|message| {
            message.get("id") == Some(&json!(1)) && message.get("result").is_some()
        });
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
