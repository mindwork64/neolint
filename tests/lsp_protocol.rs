use serde_json::{json, Value};
use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

fn frame(value: Value) -> String {
    let body = serde_json::to_string(&value).unwrap();
    format!("Content-Length: {}\r\n\r\n{}", body.len(), body)
}

fn read_frames(mut bytes: &[u8]) -> Vec<Value> {
    let mut messages = Vec::new();
    while !bytes.is_empty() {
        let header_end = bytes
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .expect("LSP frame header terminator");
        let header = std::str::from_utf8(&bytes[..header_end]).unwrap();
        let length = header
            .lines()
            .find_map(|line| line.strip_prefix("Content-Length:"))
            .unwrap()
            .trim()
            .parse::<usize>()
            .unwrap();
        let body_start = header_end + 4;
        let body_end = body_start + length;
        let value = serde_json::from_slice(&bytes[body_start..body_end]).unwrap();
        messages.push(value);
        bytes = &bytes[body_end..];
    }
    messages
}

#[test]
fn stdio_lsp_initializes_publishes_and_clears_diagnostics_then_shuts_down() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let workspace = std::env::temp_dir().join(format!("neolint-lsp-protocol-{unique}"));
    fs::create_dir_all(&workspace).unwrap();
    let source_path = workspace.join("script.lsl");
    let source_uri = url::Url::from_file_path(&source_path).unwrap().to_string();
    let workspace_uri = url::Url::from_directory_path(&workspace)
        .unwrap()
        .to_string();

    let mut input = String::new();
    input.push_str(&frame(json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "processId": null,
            "rootUri": workspace_uri,
            "capabilities": {}
        }
    })));
    input.push_str(&frame(json!({
        "jsonrpc": "2.0",
        "method": "initialized",
        "params": {}
    })));
    input.push_str(&frame(json!({
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
    })));
    input.push_str(&frame(json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didChange",
        "params": {
            "textDocument": { "uri": source_uri, "version": 2 },
            "contentChanges": [{
                "text": "default { state_entry() { llOwnerSay(\"fine\"); } }"
            }]
        }
    })));
    // A delayed old snapshot must not replace the version-2 document or its
    // diagnostics after a newer version has already been analyzed.
    input.push_str(&frame(json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didChange",
        "params": {
            "textDocument": { "uri": source_uri, "version": 1 },
            "contentChanges": [{
                "text": "default { state_entry() { integer value = ; } }"
            }]
        }
    })));
    input.push_str(&frame(json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didClose",
        "params": { "textDocument": { "uri": source_uri } }
    })));
    input.push_str(&frame(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "shutdown",
        "params": null
    })));
    input.push_str(&frame(json!({
        "jsonrpc": "2.0",
        "method": "exit",
        "params": null
    })));

    let mut child = Command::new(env!("CARGO_BIN_EXE_neolint-lsp"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start neolint-lsp binary");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "server failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let messages = read_frames(&output.stdout);
    assert!(messages.iter().any(|message| {
        message.get("id") == Some(&json!(1)) && message.get("result").is_some()
    }));
    assert!(messages.iter().any(|message| {
        message.get("id") == Some(&json!(2)) && message.get("result") == Some(&Value::Null)
    }));

    let published: Vec<_> = messages
        .iter()
        .filter(|message| message.get("method") == Some(&json!("textDocument/publishDiagnostics")))
        .collect();
    assert_eq!(published.len(), 3, "{messages:#?}");
    let first = published[0]["params"].as_object().unwrap();
    assert_eq!(first["version"], json!(1));
    assert!(!first["diagnostics"].as_array().unwrap().is_empty());
    let second = published[1]["params"].as_object().unwrap();
    assert_eq!(second["version"], json!(2));
    assert!(second["diagnostics"].as_array().unwrap().is_empty());
    let cleared = published[2]["params"].as_object().unwrap();
    assert_eq!(cleared["uri"], json!(source_uri));
    assert!(cleared["diagnostics"].as_array().unwrap().is_empty());

    fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn shared_include_diagnostics_survive_reanalysis_of_one_root() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let workspace = std::env::temp_dir().join(format!("neolint-lsp-shared-{unique}"));
    fs::create_dir_all(&workspace).unwrap();

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

    let mut input = String::new();
    input.push_str(&frame(json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "processId": null,
            "rootUri": workspace_uri,
            "capabilities": {}
        }
    })));
    input.push_str(&frame(json!({
        "jsonrpc": "2.0",
        "method": "initialized",
        "params": {}
    })));
    input.push_str(&frame(json!({
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
    })));
    input.push_str(&frame(json!({
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
    })));
    input.push_str(&frame(json!({
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
    })));

    // Root A stops including shared.lsl. Root B's distinct include-context
    // diagnostic for the same URI must remain published.
    input.push_str(&frame(json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didChange",
        "params": {
            "textDocument": { "uri": root_a_uri, "version": 2 },
            "contentChanges": [{ "text": valid_root_text }]
        }
    })));

    // When the final including root also stops referencing it, the shared
    // URI's aggregate diagnostics must be cleared.
    input.push_str(&frame(json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didChange",
        "params": {
            "textDocument": { "uri": root_b_uri, "version": 2 },
            "contentChanges": [{ "text": valid_root_text }]
        }
    })));
    input.push_str(&frame(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "shutdown",
        "params": null
    })));
    input.push_str(&frame(json!({
        "jsonrpc": "2.0",
        "method": "exit",
        "params": null
    })));

    let mut child = Command::new(env!("CARGO_BIN_EXE_neolint-lsp"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start neolint-lsp binary");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "server failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let messages = read_frames(&output.stdout);
    let shared_publications: Vec<_> = messages
        .iter()
        .filter(|message| {
            message.get("method") == Some(&json!("textDocument/publishDiagnostics"))
                && message["params"]["uri"] == json!(shared_uri)
        })
        .collect();
    assert_eq!(shared_publications.len(), 5, "{shared_publications:#?}");

    let after_root_a_reanalysis = shared_publications[3]["params"]["diagnostics"]
        .as_array()
        .unwrap();
    assert!(!after_root_a_reanalysis.is_empty());
    assert!(after_root_a_reanalysis.iter().any(|diagnostic| {
        diagnostic["relatedInformation"]
            .as_array()
            .is_some_and(|related| {
                related
                    .iter()
                    .any(|item| item["location"]["uri"] == json!(root_b_uri))
            })
    }));
    assert!(!after_root_a_reanalysis.iter().any(|diagnostic| {
        diagnostic["relatedInformation"]
            .as_array()
            .is_some_and(|related| {
                related
                    .iter()
                    .any(|item| item["location"]["uri"] == json!(root_a_uri))
            })
    }));

    let after_root_b_reanalysis = shared_publications[4]["params"]["diagnostics"]
        .as_array()
        .unwrap();
    assert!(after_root_b_reanalysis.is_empty());

    fs::remove_dir_all(workspace).unwrap();
}
