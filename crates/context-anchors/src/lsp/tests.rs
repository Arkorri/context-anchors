use std::thread;

use camino::Utf8PathBuf;
use ls_types::{ClientCapabilities, Uri, WorkspaceFolder};
use lsp_server::{Notification, Request, RequestId};

use super::*;

/// A workspace `serve` can load: `Server::new` discovers config from a real directory.
struct Fixture {
    _dir: tempfile::TempDir,
    root: Utf8PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
        std::fs::write(root.join("anchr.toml"), "[root]\nname = \"r\"\n").unwrap();
        std::fs::write(root.join("a.md"), "# title\n").unwrap();
        Self { _dir: dir, root }
    }

    fn initialize_params(&self) -> serde_json::Value {
        let uri = Uri::from_file_path(self.root.as_std_path()).unwrap();
        serde_json::json!({
            "capabilities": ClientCapabilities::default(),
            "workspaceFolders": [WorkspaceFolder { uri, name: "r".to_owned() }],
        })
    }
}

/// `handle_shutdown` answers the request and then blocks until `exit` arrives, so both must be
/// sent for the loop to end cleanly.
fn shutdown(client: &Connection, id: i32) {
    client
        .sender
        .send(Message::Request(Request::new(
            RequestId::from(id),
            "shutdown".to_owned(),
            serde_json::json!(null),
        )))
        .unwrap();
    client
        .sender
        .send(Message::Notification(Notification::new(
            "exit".to_owned(),
            serde_json::json!(null),
        )))
        .unwrap();
}

/// Drives `serve` over an in-memory connection: initialize, run `exchange`, then shut down.
/// Returns every message the server sent after initialization.
fn session(fixture: &Fixture, exchange: impl FnOnce(&Connection)) -> Vec<Message> {
    let (server_side, client) = Connection::memory();
    let worker = thread::spawn(move || serve(&server_side));

    client
        .sender
        .send(Message::Request(Request::new(
            RequestId::from(1),
            "initialize".to_owned(),
            fixture.initialize_params(),
        )))
        .unwrap();
    let initialize_response = client.receiver.recv().unwrap();
    assert!(
        matches!(&initialize_response, Message::Response(response) if response.response_result.is_ok()),
        "initialize failed: {initialize_response:?}"
    );
    client
        .sender
        .send(Message::Notification(Notification::new(
            "initialized".to_owned(),
            serde_json::json!({}),
        )))
        .unwrap();

    exchange(&client);

    shutdown(&client, 99);

    let mut seen = Vec::new();
    while let Ok(message) = client.receiver.recv() {
        seen.push(message);
    }
    drop(client);
    worker.join().unwrap().unwrap();
    seen
}

#[test]
fn initialization_advertises_the_server_by_name_and_version() {
    let fixture = Fixture::new();
    let (server_side, client) = Connection::memory();
    let worker = thread::spawn(move || serve(&server_side));

    client
        .sender
        .send(Message::Request(Request::new(
            RequestId::from(1),
            "initialize".to_owned(),
            fixture.initialize_params(),
        )))
        .unwrap();
    let Message::Response(response) = client.receiver.recv().unwrap() else {
        panic!("expected a response to initialize");
    };
    let result = response.response_result.unwrap();
    assert_eq!(result["serverInfo"]["name"], "anchr");
    assert_eq!(result["serverInfo"]["version"], env!("CARGO_PKG_VERSION"));
    assert!(
        result["capabilities"]["definitionProvider"]
            .as_bool()
            .unwrap()
    );
    assert!(result["capabilities"]["renameProvider"].as_bool().unwrap());

    // `initialize_finish` blocks until this arrives, so the handshake is not over without it.
    client
        .sender
        .send(Message::Notification(Notification::new(
            "initialized".to_owned(),
            serde_json::json!({}),
        )))
        .unwrap();
    shutdown(&client, 2);
    while client.receiver.recv().is_ok() {}
    drop(client);
    worker.join().unwrap().unwrap();
}

/// The loop must answer rather than die, so an editor sending something unsupported keeps its
/// session.
#[test]
fn an_unsupported_request_is_answered_with_an_error_and_the_session_continues() {
    let fixture = Fixture::new();
    let seen = session(&fixture, |client| {
        client
            .sender
            .send(Message::Request(Request::new(
                RequestId::from(7),
                "textDocument/nonsense".to_owned(),
                serde_json::json!({}),
            )))
            .unwrap();
    });

    let error = seen
        .iter()
        .find_map(|message| match message {
            Message::Response(response) if response.id == RequestId::from(7) => {
                response.response_result.as_ref().err()
            }
            _ => None,
        })
        .expect("the unsupported request was never answered");
    assert!(
        error.message.contains("unsupported request"),
        "{}",
        error.message
    );
}

/// Responses are the client's own traffic; the server must not treat one as work.
#[test]
fn a_response_from_the_client_is_ignored() {
    let fixture = Fixture::new();
    let seen = session(&fixture, |client| {
        client
            .sender
            .send(Message::Response(lsp_server::Response::new_ok(
                RequestId::from(1234),
                serde_json::json!(null),
            )))
            .unwrap();
    });
    assert!(
        !seen.iter().any(|message| matches!(
            message,
            Message::Response(response) if response.id == RequestId::from(1234)
        )),
        "the server answered its own client's response"
    );
}

#[test]
fn an_unparseable_notification_is_dropped_without_ending_the_session() {
    let fixture = Fixture::new();
    let seen = session(&fixture, |client| {
        client
            .sender
            .send(Message::Notification(Notification::new(
                "textDocument/didOpen".to_owned(),
                serde_json::json!({ "wrong": "shape" }),
            )))
            .unwrap();
        // Still alive: an unsupported request after the bad notification is still answered.
        client
            .sender
            .send(Message::Request(Request::new(
                RequestId::from(8),
                "textDocument/nonsense".to_owned(),
                serde_json::json!({}),
            )))
            .unwrap();
    });
    assert!(
        seen.iter().any(|message| matches!(
            message,
            Message::Response(response) if response.id == RequestId::from(8)
        )),
        "the session did not survive a malformed notification"
    );
}
