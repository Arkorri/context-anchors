//! `anchr lsp`: a synchronous Language Server Protocol server over stdio. Every message is a
//! pure function of the workspace plus the open documents, so there is no concurrency to
//! manage; one panic must not kill the editor session, so each message is isolated.

mod convert;
mod server;

use std::panic::{AssertUnwindSafe, catch_unwind};

use anyhow::Context;
use ls_types::{InitializeParams, InitializeResult, ServerInfo};
use lsp_server::{Connection, ErrorCode, Message, Response};

use self::server::Server;

pub fn run() -> anyhow::Result<()> {
    let (connection, io_threads) = Connection::stdio();

    let (initialize_id, initialize_params) = connection
        .initialize_start()
        .context("waiting for the initialize request")?;
    let params: InitializeParams =
        serde_json::from_value(initialize_params).context("reading initialize params")?;
    let mut server = Server::new(&params)?;
    let result = InitializeResult {
        capabilities: server.capabilities(),
        server_info: Some(ServerInfo {
            name: "anchr".to_owned(),
            version: Some(env!("CARGO_PKG_VERSION").to_owned()),
        }),
        offset_encoding: None,
    };
    connection
        .initialize_finish(initialize_id, serde_json::to_value(result)?)
        .context("completing initialization")?;

    for message in &connection.receiver {
        match message {
            Message::Request(request) => {
                if connection.handle_shutdown(&request)? {
                    break;
                }
                let id = request.id.clone();
                let method = request.method.clone();
                let response = match catch_unwind(AssertUnwindSafe(|| {
                    server.handle_request(request)
                })) {
                    Ok(response) => response,
                    Err(_) => {
                        eprintln!(
                            "anchr lsp: handler for {method} panicked; the request was answered with an error"
                        );
                        Response::new_err(
                            id,
                            ErrorCode::InternalError as i32,
                            format!("anchr: internal error while handling {method}"),
                        )
                    }
                };
                connection.sender.send(Message::Response(response))?;
            }
            Message::Notification(notification) => {
                let method = notification.method.clone();
                let outgoing = match catch_unwind(AssertUnwindSafe(|| {
                    server.handle_notification(notification)
                })) {
                    Ok(outgoing) => outgoing,
                    Err(_) => {
                        eprintln!(
                            "anchr lsp: handler for {method} panicked; the notification was dropped"
                        );
                        Vec::new()
                    }
                };
                for message in outgoing {
                    connection.sender.send(message)?;
                }
            }
            Message::Response(_) => {}
        }
    }

    // The writer thread ends only once the connection (and with it the sender) is gone.
    drop(connection);
    io_threads.join().context("shutting down stdio threads")?;
    Ok(())
}
