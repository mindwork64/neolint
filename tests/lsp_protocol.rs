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
        Self::with_capabilities(workspace_uri, supports_file_operations, true)
    }

    fn with_completion_snippets(workspace_uri: &str, supports_snippets: bool) -> Self {
        Self::with_capabilities(workspace_uri, true, supports_snippets)
    }

    fn with_capabilities(
        workspace_uri: &str,
        supports_file_operations: bool,
        supports_snippets: bool,
    ) -> Self {
        let mut capabilities = json!({
            "textDocument": {
                "completion": {
                    "completionItem": { "snippetSupport": supports_snippets }
                }
            }
        });
        if supports_file_operations {
            capabilities["workspace"] = json!({
                "fileOperations": {
                    "didCreate": true,
                    "didRename": true,
                    "didDelete": true
                }
            });
        }
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
        assert_eq!(
            initialize_result["result"]["hoverProvider"], true,
            "the server must advertise textDocument/hover"
        );
        assert_eq!(
            initialize_result["result"]["definitionProvider"], true,
            "the server must advertise textDocument/definition"
        );
        assert!(
            initialize_result["result"]["completionProvider"].is_object(),
            "the server must advertise textDocument/completion"
        );
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

    fn request_position(
        &mut self,
        id: u64,
        method: &str,
        uri: &str,
        line: u32,
        character: u32,
    ) -> Value {
        self.send(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": {
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character }
            }
        }));
        let messages = self.receive_until(|message| message.get("id") == Some(&json!(id)));
        messages
            .into_iter()
            .find(|message| message.get("id") == Some(&json!(id)))
            .expect("LSP request response")
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

fn position_of(source: &str, offset: usize) -> (u32, u32) {
    let prefix = &source[..offset];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count() as u32;
    let column_text = prefix.rsplit('\n').next().unwrap_or_default();
    let character = column_text.encode_utf16().count() as u32;
    (line, character)
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
fn hover_and_definition_resolve_current_script_symbols_and_standard_api() {
    let workspace = unique_workspace("neolint-lsp-hover-definition");
    let source = concat!(
        "// llOwnerSay is documented below\n",
        "integer counter = 1;\n",
        "integer add(integer left, integer right) { return left + right; }\n",
        "default { state_entry() { integer local = add(counter, 2); llOwnerSay((string)local); } }\n",
    );
    let root_path = workspace.join("main.lsl");
    fs::write(&root_path, source).unwrap();
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
                "text": source
            }
        }
    }));
    client.receive_until(|message| is_publish(message, &root_uri, Some(1)));

    let add_call = source.match_indices("add(").nth(1).unwrap().0;
    let (line, character) = position_of(source, add_call);
    let hover = client.request_position(10, "textDocument/hover", &root_uri, line, character);
    assert!(hover["error"].is_null(), "hover error: {hover:#?}");
    let hover_text = hover["result"]["contents"]["value"].as_str().unwrap();
    assert!(
        hover_text.contains("integer add(integer left, integer right)"),
        "{hover_text}"
    );

    let (line, character) = position_of(source, add_call);
    let definition =
        client.request_position(11, "textDocument/definition", &root_uri, line, character);
    assert!(
        definition["error"].is_null(),
        "definition error: {definition:#?}"
    );
    let declaration = source.find("add(").unwrap();
    let (expected_line, expected_character) = position_of(source, declaration);
    assert_eq!(definition["result"]["uri"], root_uri);
    assert_eq!(
        definition["result"]["range"]["start"]["line"],
        expected_line
    );
    assert_eq!(
        definition["result"]["range"]["start"]["character"],
        expected_character
    );

    let parameter_use = source.find("left +").unwrap();
    let (line, character) = position_of(source, parameter_use);
    let hover = client.request_position(16, "textDocument/hover", &root_uri, line, character);
    let parameter_hover = hover["result"]["contents"]["value"].as_str().unwrap();
    assert!(
        parameter_hover.contains("integer left"),
        "{parameter_hover}"
    );
    let definition =
        client.request_position(17, "textDocument/definition", &root_uri, line, character);
    let parameter_declaration = source.find("left,").unwrap();
    let (expected_line, expected_character) = position_of(source, parameter_declaration);
    assert_eq!(
        definition["result"]["range"]["start"]["line"],
        expected_line
    );
    assert_eq!(
        definition["result"]["range"]["start"]["character"],
        expected_character
    );

    let counter_use = source.match_indices("counter").nth(1).unwrap().0;
    let (line, character) = position_of(source, counter_use);
    let definition =
        client.request_position(12, "textDocument/definition", &root_uri, line, character);
    let counter_declaration = source.find("counter").unwrap();
    let (expected_line, expected_character) = position_of(source, counter_declaration);
    assert_eq!(
        definition["result"]["range"]["start"]["line"],
        expected_line
    );
    assert_eq!(
        definition["result"]["range"]["start"]["character"],
        expected_character
    );

    let (line, character) = position_of(source, counter_use);
    let hover = client.request_position(18, "textDocument/hover", &root_uri, line, character);
    let counter_hover = hover["result"]["contents"]["value"].as_str().unwrap();
    assert!(counter_hover.contains("integer counter"), "{counter_hover}");

    let local_use = source.match_indices("local").nth(1).unwrap().0;
    let (line, character) = position_of(source, local_use);
    let definition =
        client.request_position(13, "textDocument/definition", &root_uri, line, character);
    let local_declaration = source.find("local").unwrap();
    let (expected_line, expected_character) = position_of(source, local_declaration);
    assert_eq!(
        definition["result"]["range"]["start"]["line"],
        expected_line
    );
    assert_eq!(
        definition["result"]["range"]["start"]["character"],
        expected_character
    );

    let (line, character) = position_of(source, local_use);
    let hover = client.request_position(19, "textDocument/hover", &root_uri, line, character);
    let local_hover = hover["result"]["contents"]["value"].as_str().unwrap();
    assert!(local_hover.contains("integer local"), "{local_hover}");

    let api_use = source.match_indices("llOwnerSay").nth(1).unwrap().0;
    let (line, character) = position_of(source, api_use);
    let hover = client.request_position(14, "textDocument/hover", &root_uri, line, character);
    let api_text = hover["result"]["contents"]["value"].as_str().unwrap();
    assert!(
        api_text.contains("void llOwnerSay(string msg)"),
        "{api_text}"
    );

    let comment_use = source.find("llOwnerSay").unwrap();
    let (line, character) = position_of(source, comment_use);
    let hover = client.request_position(15, "textDocument/hover", &root_uri, line, character);
    assert!(
        hover["result"].is_null(),
        "comments must not produce symbol hover: {hover:#?}"
    );
    let (line, character) = position_of(source, api_use);
    let definition =
        client.request_position(20, "textDocument/definition", &root_uri, line, character);
    assert!(
        definition["result"].is_null(),
        "built-in functions do not have source definitions: {definition:#?}"
    );

    // A new document revision must atomically replace the old index.
    let updated_source = concat!(
        "integer counter = 1;\n",
        "float add(float left, float right) { return left + right; }\n",
        "default { state_entry() { float local = add((float)counter, 2.0); llOwnerSay((string)local); } }\n",
    );
    client.send(json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didChange",
        "params": {
            "textDocument": { "uri": root_uri, "version": 2 },
            "contentChanges": [{ "text": updated_source }]
        }
    }));
    client.receive_until(|message| is_publish(message, &root_uri, Some(2)));
    let updated_call = updated_source.match_indices("add(").nth(1).unwrap().0;
    let (line, character) = position_of(updated_source, updated_call);
    let hover = client.request_position(21, "textDocument/hover", &root_uri, line, character);
    let updated_hover = hover["result"]["contents"]["value"].as_str().unwrap();
    assert!(
        updated_hover.contains("float add(float left, float right)"),
        "{updated_hover}"
    );

    client.shutdown();
    fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn hover_and_definition_ignore_vector_component_selectors() {
    let workspace = unique_workspace("neolint-lsp-component-selector");
    let source = concat!(
        "integer x = 7;\n",
        "vector position;\n",
        "default { state_entry() { float component = position.x; llOwnerSay((string)x); } }\n",
    );
    let root_path = workspace.join("main.lsl");
    fs::write(&root_path, source).unwrap();
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
                "text": source
            }
        }
    }));
    client.receive_until(|message| is_publish(message, &root_uri, Some(1)));

    let component_offset = source.find("position.x").unwrap() + "position.".len();
    let (line, character) = position_of(source, component_offset);
    let hover = client.request_position(10, "textDocument/hover", &root_uri, line, character);
    assert!(
        hover["result"].is_null(),
        "component selectors are not variable bindings: {hover:#?}"
    );
    let definition =
        client.request_position(11, "textDocument/definition", &root_uri, line, character);
    assert!(
        definition["result"].is_null(),
        "component selectors have no declaration target: {definition:#?}"
    );

    let scalar_use = source.rfind("x)").unwrap();
    let (line, character) = position_of(source, scalar_use);
    let hover = client.request_position(12, "textDocument/hover", &root_uri, line, character);
    let value = hover["result"]["contents"]["value"].as_str().unwrap();
    assert!(
        value.contains("integer x"),
        "ordinary variable hover must remain available: {value}"
    );
    let definition =
        client.request_position(13, "textDocument/definition", &root_uri, line, character);
    let declaration = source.find("x =").unwrap();
    let (expected_line, expected_character) = position_of(source, declaration);
    assert_eq!(
        definition["result"]["range"]["start"]["line"],
        expected_line
    );
    assert_eq!(
        definition["result"]["range"]["start"]["character"],
        expected_character
    );

    client.shutdown();
    fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn hover_and_definition_follow_the_active_preprocessed_script() {
    let workspace = unique_workspace("neolint-lsp-hover-preprocessed-symbols");
    fs::write(workspace.join(".neolint.conf"), "preprocess = true\n").unwrap();
    let source = concat!(
        "#if 0\n",
        "integer hidden = 1;\n",
        "#endif\n",
        "#define DECLARE_COUNTER integer expanded = 7;\n",
        "DECLARE_COUNTER\n",
        "default { state_entry() { llOwnerSay((string)expanded); } }\n",
    );
    let root_path = workspace.join("main.lsl");
    fs::write(&root_path, source).unwrap();
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
                "text": source
            }
        }
    }));
    client.receive_until(|message| is_publish(message, &root_uri, Some(1)));

    let hidden_offset = source.find("hidden").unwrap();
    let (line, character) = position_of(source, hidden_offset);
    let hover = client.request_position(10, "textDocument/hover", &root_uri, line, character);
    assert!(
        hover["result"].is_null(),
        "declarations removed by #if must not be indexed: {hover:#?}"
    );
    let definition =
        client.request_position(11, "textDocument/definition", &root_uri, line, character);
    assert!(definition["result"].is_null());

    let use_offset = source.rfind("expanded").unwrap();
    let (line, character) = position_of(source, use_offset);
    let hover = client.request_position(12, "textDocument/hover", &root_uri, line, character);
    let hover_text = hover["result"]["contents"]["value"].as_str().unwrap();
    assert!(
        hover_text.contains("integer expanded"),
        "macro-expanded declaration must participate in symbol resolution: {hover_text}"
    );
    let definition =
        client.request_position(13, "textDocument/definition", &root_uri, line, character);
    assert_eq!(definition["result"]["uri"], root_uri);
    assert!(
        definition["result"]["range"]["start"]["line"].is_number(),
        "macro-expanded declaration should map to an original root location: {definition:#?}"
    );

    let completion =
        client.request_position(14, "textDocument/completion", &root_uri, line, character);
    let completion_items = completion["result"].as_array().unwrap();
    assert!(
        completion_items
            .iter()
            .any(|item| item["label"] == "expanded"),
        "macro-expanded declaration should be available to completion: {completion:#?}"
    );
    assert!(
        !completion_items
            .iter()
            .any(|item| item["label"] == "hidden"),
        "declarations in inactive #if branches must not be suggested: {completion:#?}"
    );

    client.shutdown();
    fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn hover_uses_the_selected_opensim_runtime_api_catalogue() {
    let workspace = unique_workspace("neolint-lsp-hover-opensim");
    fs::write(
        workspace.join(".neolint.conf"),
        "target = opensim-xengine\n",
    )
    .unwrap();
    let source = "default { state_entry() { osGetAvatarList(); } }\n";
    let root_path = workspace.join("main.lsl");
    fs::write(&root_path, source).unwrap();
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
                "text": source
            }
        }
    }));
    client.receive_until(|message| is_publish(message, &root_uri, Some(1)));

    let offset = source.find("osGetAvatarList").unwrap();
    let (line, character) = position_of(source, offset);
    let hover = client.request_position(10, "textDocument/hover", &root_uri, line, character);
    let value = hover["result"]["contents"]["value"].as_str().unwrap();
    assert!(value.contains("list osGetAvatarList()"), "{value}");

    client.shutdown();
    fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn hover_and_definition_use_effective_firestorm_switch_parser_options() {
    let workspace = unique_workspace("neolint-lsp-hover-firestorm-switch");
    let include_path = workspace.join("switches.inc");
    fs::write(&include_path, "#define USE_SWITCHES\n").unwrap();
    fs::write(
        workspace.join(".neolint.conf"),
        "preprocess = true\ndialect = firestorm\n",
    )
    .unwrap();
    let source = concat!(
        "#include \"switches.inc\"\n",
        "integer selector; default { state_entry() { switch (selector) { case 1: integer inside = 42; llOwnerSay((string)inside); break; default: break; } } }\n",
    );
    let root_path = workspace.join("main.lsl");
    fs::write(&root_path, source).unwrap();
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
                "text": source
            }
        }
    }));
    let messages = client.receive_until(|message| is_publish(message, &root_uri, Some(1)));
    assert!(
        messages.iter().any(|message| {
            is_publish(message, &root_uri, Some(1))
                && message["params"]["diagnostics"]
                    .as_array()
                    .is_some_and(Vec::is_empty)
        }),
        "Firestorm fixture should analyze without diagnostics: {messages:#?}"
    );

    let use_offset = source.match_indices("inside").nth(1).unwrap().0;
    let (line, character) = position_of(source, use_offset);
    let hover = client.request_position(10, "textDocument/hover", &root_uri, line, character);
    let value = hover["result"]["contents"]["value"].as_str().unwrap();
    assert!(value.contains("integer inside"), "{value}");
    let definition =
        client.request_position(11, "textDocument/definition", &root_uri, line, character);
    let declaration = source.find("inside").unwrap();
    let (expected_line, expected_character) = position_of(source, declaration);
    assert_eq!(
        definition["result"]["range"]["start"]["line"],
        expected_line
    );
    assert_eq!(
        definition["result"]["range"]["start"]["character"],
        expected_character
    );

    client.shutdown();
    fs::remove_dir_all(workspace).unwrap();
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

#[test]
fn completion_offers_visible_symbols_target_builtins_and_replacement_edits() {
    let workspace = unique_workspace("neolint-lsp-completion");
    let source = concat!(
        "// llGetTime is not a completion context\n",
        "/* llGetTime is not a completion context either */\n",
        "string note = \"llGetTime\";\n",
        "integer global_count = 1;\n",
        "integer x = 1;\n",
        "vector position = ZERO_VECTOR;\n",
        "integer helper(integer value) { integer helper_local = value; return helper_local; }\n",
        "default {\n",
        "    state_entry() {\n",
        "        integer local_count = 0;\n",
        "        llGetTime();\n",
        "        integer future_local = 2;\n",
        "        position.x;\n",
        "    }\n",
        "}\n",
    );
    let root_path = workspace.join("main.lsl");
    fs::write(&root_path, source).unwrap();
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
                "text": source
            }
        }
    }));
    client.receive_until(|message| is_publish(message, &root_uri, Some(1)));

    let local_declaration = source.find("local_count").unwrap();
    let local_statement_end = local_declaration + "local_count = 0;".len();
    let (line, character) = position_of(source, local_statement_end);
    let completion =
        client.request_position(30, "textDocument/completion", &root_uri, line, character);
    assert!(
        completion["error"].is_null(),
        "completion error: {completion:#?}"
    );
    let items = completion["result"]
        .as_array()
        .expect("completion item array");
    let labels: Vec<_> = items
        .iter()
        .filter_map(|item| item["label"].as_str())
        .collect();
    for expected in [
        "local_count",
        "global_count",
        "helper",
        "llGetTime",
        "TRUE",
        "state_entry",
    ] {
        assert!(
            labels.contains(&expected),
            "missing {expected} in {labels:?}"
        );
    }
    assert!(
        !labels.contains(&"helper_local"),
        "a local from another function must not leak into this scope: {labels:?}"
    );
    assert!(
        !labels.contains(&"future_local"),
        "a local declared after the cursor must not be suggested: {labels:?}"
    );

    let helper = items
        .iter()
        .find(|item| item["label"] == "helper")
        .expect("user function completion");
    assert_eq!(helper["kind"], 3, "helper should be a function completion");
    assert_eq!(helper["detail"], "integer helper(integer value)");
    assert_eq!(helper["textEdit"]["newText"], "helper(${1:value})");
    assert_eq!(helper["insertTextFormat"], 2, "function calls use snippets");

    let builtin = items
        .iter()
        .find(|item| item["label"] == "llGetTime")
        .expect("built-in function completion");
    assert_eq!(builtin["textEdit"]["newText"], "llGetTime()");

    // Function parameters and locals are visible inside their own body, but
    // do not leak into another event's scope.
    let helper_body_cursor =
        source.find("helper_local = value;").unwrap() + "helper_local = value;".len();
    let (line, character) = position_of(source, helper_body_cursor);
    let helper_scope =
        client.request_position(37, "textDocument/completion", &root_uri, line, character);
    let helper_labels: Vec<_> = helper_scope["result"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|item| item["label"].as_str())
        .collect();
    assert!(
        helper_labels.contains(&"value"),
        "parameter missing: {helper_labels:?}"
    );
    assert!(
        helper_labels.contains(&"helper_local"),
        "local missing: {helper_labels:?}"
    );
    assert!(
        !helper_labels.contains(&"local_count"),
        "another event's local leaked: {helper_labels:?}"
    );

    // A completion in the middle of a word replaces the entire identifier,
    // not only the prefix before the cursor.
    let cursor = local_declaration + "local".len();
    let (line, character) = position_of(source, cursor);
    let partial =
        client.request_position(31, "textDocument/completion", &root_uri, line, character);
    let local_item = partial["result"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["label"] == "local_count")
        .expect("prefix-matched local variable");
    let (expected_line, expected_character) = position_of(source, local_declaration);
    assert_eq!(
        local_item["textEdit"]["range"]["start"]["line"],
        expected_line
    );
    assert_eq!(
        local_item["textEdit"]["range"]["start"]["character"],
        expected_character
    );
    assert_eq!(local_item["textEdit"]["newText"], "local_count");

    // Strings and both comment forms are deliberately not completion contexts.
    let comment_offset = source.find("// llGetTime").unwrap() + 3;
    let (line, character) = position_of(source, comment_offset);
    let comment =
        client.request_position(32, "textDocument/completion", &root_uri, line, character);
    assert_eq!(comment["result"], json!([]));

    let string_offset = source.match_indices("llGetTime").nth(2).unwrap().0 + 2;
    let (line, character) = position_of(source, string_offset);
    let string = client.request_position(33, "textDocument/completion", &root_uri, line, character);
    assert_eq!(string["result"], json!([]));

    let block_comment_offset = source.find("/* llGetTime").unwrap() + 4;
    let (line, character) = position_of(source, block_comment_offset);
    let block_comment =
        client.request_position(35, "textDocument/completion", &root_uri, line, character);
    assert_eq!(block_comment["result"], json!([]));

    // `position.x` is a vector component selector, not a reference to the
    // unrelated global integer named `x`.
    let selector_offset = source.find("position.x").unwrap() + "position.x".len();
    let (line, character) = position_of(source, selector_offset);
    let selector =
        client.request_position(36, "textDocument/completion", &root_uri, line, character);
    assert_eq!(selector["result"], json!([]));

    client.shutdown();
    fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn completion_uses_the_selected_opensim_catalogue_and_marks_ossl_permissions() {
    let workspace = unique_workspace("neolint-lsp-completion-opensim");
    fs::write(
        workspace.join(".neolint.conf"),
        "target = opensim-xengine\n",
    )
    .unwrap();
    let source = "default { state_entry() { osGetAvatarList(); } }\n";
    let root_path = workspace.join("main.lsl");
    fs::write(&root_path, source).unwrap();
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
                "text": source
            }
        }
    }));
    client.receive_until(|message| is_publish(message, &root_uri, Some(1)));

    let cursor = source.find("osGetAvatarList").unwrap() + "osGetA".len();
    let (line, character) = position_of(source, cursor);
    let completion =
        client.request_position(34, "textDocument/completion", &root_uri, line, character);
    assert!(
        completion["error"].is_null(),
        "completion error: {completion:#?}"
    );
    let item = completion["result"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["label"] == "osGetAvatarList")
        .expect("OpenSim OSSL completion");
    assert!(item["detail"].as_str().unwrap().contains("OSSL"));
    assert!(item["detail"]
        .as_str()
        .unwrap()
        .contains("availability depends on region policy"));
    // The source already has an opening parenthesis after the cursor word,
    // so completion must not duplicate it.
    assert_eq!(item["textEdit"]["newText"], "osGetAvatarList");

    client.shutdown();
    fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn completion_uses_plain_text_calls_when_client_does_not_support_snippets() {
    let workspace = unique_workspace("neolint-lsp-completion-no-snippets");
    let source = concat!(
        "integer helper(integer value) { return value; }\n",
        "default { state_entry() { integer local_value = 1; } }\n",
    );
    let root_path = workspace.join("main.lsl");
    fs::write(&root_path, source).unwrap();
    let root_uri = url::Url::from_file_path(&root_path).unwrap().to_string();
    let workspace_uri = url::Url::from_directory_path(&workspace)
        .unwrap()
        .to_string();
    let mut client = LspClient::with_completion_snippets(&workspace_uri, false);
    client.send(json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": root_uri,
                "languageId": "lsl",
                "version": 1,
                "text": source
            }
        }
    }));
    client.receive_until(|message| is_publish(message, &root_uri, Some(1)));

    let cursor = source.find("local_value").unwrap() + "local_value = 1;".len();
    let (line, character) = position_of(source, cursor);
    let completion =
        client.request_position(40, "textDocument/completion", &root_uri, line, character);
    let item = completion["result"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["label"] == "helper")
        .expect("user function completion");
    assert_eq!(item["textEdit"]["newText"], "helper(value)");
    assert_eq!(item["insertTextFormat"], 1, "plain-text completion format");
    let builtin = completion["result"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["label"] == "llGetTime")
        .expect("built-in completion");
    assert_eq!(builtin["textEdit"]["newText"], "llGetTime()");
    assert_eq!(builtin["insertTextFormat"], 1);

    client.shutdown();
    fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn completion_preserves_scope_ranges_across_preprocessor_source_gaps() {
    let workspace = unique_workspace("neolint-lsp-completion-preprocessor-scope");
    fs::write(workspace.join(".neolint.conf"), "preprocess = true\n").unwrap();
    let source = concat!(
        "integer helper(integer value) {\n",
        "    integer before_directive = value;\n",
        "#if 1\n",
        "    integer inside_directive = value;\n",
        "#endif\n",
        "    integer after_directive = value;\n",
        "    return after_directive;\n",
        "}\n",
        "default { state_entry() {} }\n",
    );
    let root_path = workspace.join("main.lsl");
    fs::write(&root_path, source).unwrap();
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
                "text": source
            }
        }
    }));
    client.receive_until(|message| is_publish(message, &root_uri, Some(1)));

    let cursor =
        source.find("after_directive = value;").unwrap() + "after_directive = value;".len();
    let (line, character) = position_of(source, cursor);
    let completion =
        client.request_position(41, "textDocument/completion", &root_uri, line, character);
    assert!(
        completion["error"].is_null(),
        "completion error: {completion:#?}"
    );
    let labels: Vec<_> = completion["result"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|item| item["label"].as_str())
        .collect();
    for expected in [
        "value",
        "before_directive",
        "inside_directive",
        "after_directive",
    ] {
        assert!(
            labels.contains(&expected),
            "missing {expected} after a preprocessor source gap: {labels:?}"
        );
    }

    client.shutdown();
    fs::remove_dir_all(workspace).unwrap();
}
