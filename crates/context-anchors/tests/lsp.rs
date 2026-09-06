//! Drives `anchr lsp` over stdio with a minimal JSON-RPC client. Reads run on a thread with a
//! timeout so a protocol mistake fails the test instead of hanging it.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::needless_pass_by_value
)]

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use serde_json::{Value, json};

const TIMEOUT: Duration = Duration::from_secs(30);

struct Client {
    child: Child,
    stdin: ChildStdin,
    incoming: Receiver<Value>,
    next_id: i64,
    notifications: Vec<Value>,
}

impl Client {
    fn start(root: &std::path::Path) -> Self {
        let mut child = Command::new(assert_cmd::cargo::cargo_bin("anchr"))
            .arg("lsp")
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (sender, incoming) = channel();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            while let Some(message) = read_message(&mut reader) {
                eprintln!("<- {message}");
                if sender.send(message).is_err() {
                    break;
                }
            }
        });
        Self {
            child,
            stdin,
            incoming,
            next_id: 1,
            notifications: Vec::new(),
        }
    }

    fn send(&mut self, message: Value) {
        let body = serde_json::to_string(&message).unwrap();
        eprintln!("-> {body}");
        write!(self.stdin, "Content-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        self.stdin.flush().unwrap();
    }

    fn notify(&mut self, method: &str, params: Value) {
        self.send(json!({ "jsonrpc": "2.0", "method": method, "params": params }));
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        loop {
            let message = self.next_message(method);
            if message.get("id") == Some(&json!(id)) {
                return message;
            }
            self.notifications.push(message);
        }
    }

    fn next_message(&mut self, waiting_for: &str) -> Value {
        self.incoming
            .recv_timeout(TIMEOUT)
            .unwrap_or_else(|_| panic!("timed out waiting for {waiting_for}"))
    }

    /// Waits until a publishDiagnostics notification for `uri` arrives.
    fn diagnostics_for(&mut self, uri: &str) -> Vec<Value> {
        loop {
            if let Some(index) = self.notifications.iter().position(|n| {
                n["method"] == "textDocument/publishDiagnostics" && n["params"]["uri"] == uri
            }) {
                let notification = self.notifications.remove(index);
                return notification["params"]["diagnostics"]
                    .as_array()
                    .unwrap()
                    .clone();
            }
            let message = self.next_message("publishDiagnostics");
            self.notifications.push(message);
        }
    }

    fn shutdown(mut self) {
        self.request("shutdown", Value::Null);
        self.notify("exit", Value::Null);
        let status = self.child.wait().unwrap();
        assert!(status.success(), "server exited with {status}");
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn read_message(reader: &mut impl BufRead) -> Option<Value> {
    let mut length = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).ok()? == 0 {
            return None;
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some(value) = line.strip_prefix("Content-Length: ") {
            length = Some(value.parse::<usize>().ok()?);
        }
    }
    let mut body = vec![0; length?];
    reader.read_exact(&mut body).ok()?;
    serde_json::from_slice(&body).ok()
}

/// Built the way the server builds its own URIs, so the two compare equal on every platform;
/// `dunce` drops the `\\?\` prefix Windows canonicalization adds.
fn file_uri(path: &std::path::Path) -> String {
    let canonical = dunce::canonicalize(path).unwrap();
    ls_types::Uri::from_file_path(&canonical)
        .unwrap()
        .as_str()
        .to_owned()
}

#[test]
fn the_server_publishes_diagnostics_and_answers_navigation_requests() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("repo");
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::write(
        root.join("docs/guide.md"),
        "# Guide @anchor[guide]\n\nSee @ref[#guide] and @ref[docs/other.md#missing-here].\n",
    )
    .unwrap();
    std::fs::write(
        root.join("docs/other.md"),
        "Refers to @ref[#guide] and @ref[#gone].\n",
    )
    .unwrap();
    let guide_uri = file_uri(&root.join("docs/guide.md"));
    let other_uri = file_uri(&root.join("docs/other.md"));

    let mut client = Client::start(&root);
    let initialize = client.request(
        "initialize",
        json!({
            "processId": null,
            "rootUri": file_uri(&root),
            "capabilities": { "general": { "positionEncodings": ["utf-8"] } },
        }),
    );
    let capabilities = &initialize["result"]["capabilities"];
    assert_eq!(capabilities["positionEncoding"], "utf-8");
    assert_eq!(capabilities["textDocumentSync"], 1);
    assert_eq!(capabilities["renameProvider"], true);
    client.notify("initialized", json!({}));

    let other_text = std::fs::read_to_string(root.join("docs/other.md")).unwrap();
    client.notify(
        "textDocument/didOpen",
        json!({ "textDocument": { "uri": other_uri, "languageId": "markdown", "version": 1, "text": other_text } }),
    );
    let diagnostics = client.diagnostics_for(&other_uri);
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0]["code"], "anchor-missing");
    assert_eq!(diagnostics[0]["severity"], 1);
    assert_eq!(
        diagnostics[0]["range"]["start"],
        json!({ "line": 0, "character": 27 })
    );

    // @noref[guide.md, other.md]
    // Go to definition from the reference in other.md lands on the anchor in guide.md.
    let definition = client.request(
        "textDocument/definition",
        json!({ "textDocument": { "uri": other_uri }, "position": { "line": 0, "character": 12 } }),
    );
    let locations = definition["result"].as_array().unwrap();
    assert_eq!(locations.len(), 1);
    assert_eq!(locations[0]["uri"], guide_uri);
    assert_eq!(
        locations[0]["range"]["start"],
        json!({ "line": 0, "character": 8 })
    );

    // References to the anchor, from the anchor itself, including the declaration.
    let references = client.request(
        "textDocument/references",
        json!({
            "textDocument": { "uri": guide_uri },
            "position": { "line": 0, "character": 10 },
            "context": { "includeDeclaration": true },
        }),
    );
    assert_eq!(references["result"].as_array().unwrap().len(), 3);

    // Document symbols list anchors.
    let symbols = client.request(
        "textDocument/documentSymbol",
        json!({ "textDocument": { "uri": guide_uri } }),
    );
    let symbols = symbols["result"].as_array().unwrap();
    assert_eq!(symbols.len(), 1);
    assert_eq!(symbols[0]["name"], "guide");

    // Rename produces edits in both files for exactly the id bytes.
    let rename = client.request(
        "textDocument/rename",
        json!({
            "textDocument": { "uri": guide_uri },
            "position": { "line": 0, "character": 10 },
            "newName": "user-guide",
        }),
    );
    let changes = rename["result"]["changes"].as_object().unwrap();
    assert_eq!(changes.len(), 2);
    let guide_edits = changes[&guide_uri].as_array().unwrap();
    assert_eq!(guide_edits.len(), 2);
    assert!(guide_edits.iter().all(|e| e["newText"] == "user-guide"));
    assert_eq!(
        guide_edits[0]["range"],
        json!({ "start": { "line": 0, "character": 16 }, "end": { "line": 0, "character": 21 } })
    );

    // An invalid new name is refused with InvalidParams.
    let refused = client.request(
        "textDocument/rename",
        json!({
            "textDocument": { "uri": guide_uri },
            "position": { "line": 0, "character": 10 },
            "newName": "has space",
        }),
    );
    assert_eq!(refused["error"]["code"], -32602);

    // Editing the buffer to remove the anchor makes other.md's reference to it an error too.
    client.notify(
        "textDocument/didOpen",
        json!({ "textDocument": { "uri": guide_uri, "languageId": "markdown", "version": 1, "text": "# Guide\n" } }),
    );
    let after_edit = client.diagnostics_for(&other_uri);
    assert_eq!(after_edit.len(), 2, "{after_edit:?}");

    client.shutdown();
}

#[test]
fn alias_uses_navigate_through_their_declaration_and_rename_within_their_file() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("repo");
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::write(root.join("docs/guide.md"), "# Guide @anchor[guide]\n").unwrap();
    std::fs::write(
        root.join("docs/other.md"),
        "@ref[#guide as G] then @[G] and @[G].\n@ref[#gone as Gone] @[Gone]\n",
    )
    .unwrap();
    let guide_uri = file_uri(&root.join("docs/guide.md"));
    let other_uri = file_uri(&root.join("docs/other.md"));

    let mut client = Client::start(&root);
    client.request(
        "initialize",
        json!({
            "processId": null,
            "rootUri": file_uri(&root),
            "capabilities": { "general": { "positionEncodings": ["utf-8"] } },
        }),
    );
    client.notify("initialized", json!({}));
    let other_text = std::fs::read_to_string(root.join("docs/other.md")).unwrap();
    client.notify(
        "textDocument/didOpen",
        json!({ "textDocument": { "uri": other_uri, "languageId": "markdown", "version": 1, "text": other_text } }),
    );

    // The broken declaration is the only diagnostic; its uses ride along as related information.
    let diagnostics = client.diagnostics_for(&other_uri);
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0]["code"], "anchor-missing");
    assert_eq!(
        diagnostics[0]["range"]["start"],
        json!({ "line": 1, "character": 0 })
    );
    assert!(
        diagnostics[0]["message"]
            .as_str()
            .unwrap()
            .contains("has 1 use")
    );
    let related = diagnostics[0]["relatedInformation"].as_array().unwrap();
    assert_eq!(related.len(), 1);
    assert_eq!(
        related[0]["location"]["range"]["start"],
        json!({ "line": 1, "character": 20 })
    );

    // Definition from a use: the target, then the declaration in the same file.
    let definition = client.request(
        "textDocument/definition",
        json!({ "textDocument": { "uri": other_uri }, "position": { "line": 0, "character": 25 } }),
    );
    let locations = definition["result"].as_array().unwrap();
    assert_eq!(locations.len(), 2, "{locations:?}");
    assert_eq!(locations[0]["uri"], guide_uri);
    assert_eq!(locations[1]["uri"], other_uri);
    assert_eq!(
        locations[1]["range"]["start"],
        json!({ "line": 0, "character": 0 })
    );

    // References from the declaration include its uses.
    let references = client.request(
        "textDocument/references",
        json!({
            "textDocument": { "uri": other_uri },
            "position": { "line": 0, "character": 6 },
            "context": { "includeDeclaration": false },
        }),
    );
    assert_eq!(references["result"].as_array().unwrap().len(), 3);

    // Document symbols list alias declarations.
    let symbols = client.request(
        "textDocument/documentSymbol",
        json!({ "textDocument": { "uri": other_uri } }),
    );
    let names: Vec<&str> = symbols["result"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["G", "Gone"]);

    // Rename on a use rewrites the alias token and every use, in this file only.
    let rename = client.request(
        "textDocument/rename",
        json!({
            "textDocument": { "uri": other_uri },
            "position": { "line": 0, "character": 25 },
            "newName": "Guide",
        }),
    );
    let changes = rename["result"]["changes"].as_object().unwrap();
    assert_eq!(changes.len(), 1, "{changes:?}");
    let edits = changes[&other_uri].as_array().unwrap();
    assert_eq!(edits.len(), 3);
    assert!(edits.iter().all(|e| e["newText"] == "Guide"));
    assert_eq!(
        edits[0]["range"],
        json!({ "start": { "line": 0, "character": 15 }, "end": { "line": 0, "character": 16 } })
    );

    // Rename with the cursor on the anchor id renames the anchor, in both files.
    let rename = client.request(
        "textDocument/rename",
        json!({
            "textDocument": { "uri": other_uri },
            "position": { "line": 0, "character": 7 },
            "newName": "user-guide",
        }),
    );
    let changes = rename["result"]["changes"].as_object().unwrap();
    assert_eq!(changes.len(), 2, "{changes:?}");
    assert_eq!(
        changes[&other_uri][0]["range"],
        json!({ "start": { "line": 0, "character": 6 }, "end": { "line": 0, "character": 11 } })
    );

    // An invalid alias is refused; so is a name the file already declares.
    for bad in ["has space", "Gone"] {
        let refused = client.request(
            "textDocument/rename",
            json!({
                "textDocument": { "uri": other_uri },
                "position": { "line": 0, "character": 25 },
                "newName": bad,
            }),
        );
        assert_eq!(refused["error"]["code"], -32602, "{bad}");
    }

    client.shutdown();
}
