use ls_types::{
    ClientCapabilities, GeneralClientCapabilities, PositionEncodingKind, WorkspaceFolder,
};
use lsp_server::RequestId;

use super::*;

fn folder(uri: Uri) -> WorkspaceFolder {
    WorkspaceFolder {
        uri,
        name: "r".to_owned(),
    }
}

fn file_uri(path: &str) -> Uri {
    let absolute = if cfg!(windows) {
        format!(r"C:\{}", path.replace('/', r"\"))
    } else {
        format!("/{path}")
    };
    Uri::from_file_path(std::path::Path::new(&absolute)).unwrap()
}

fn params_offering(encodings: Option<Vec<PositionEncodingKind>>) -> InitializeParams {
    InitializeParams {
        capabilities: ClientCapabilities {
            general: Some(GeneralClientCapabilities {
                position_encodings: encodings,
                ..GeneralClientCapabilities::default()
            }),
            ..ClientCapabilities::default()
        },
        ..InitializeParams::default()
    }
}

#[test]
fn utf8_positions_are_used_only_when_the_client_advertises_them() {
    let offered = params_offering(Some(vec![
        PositionEncodingKind::UTF16,
        PositionEncodingKind::UTF8,
    ]));
    assert_eq!(negotiated_encoding(&offered), PositionEncoding::Utf8);
}

/// Every path that does not positively advertise UTF-8 must land on UTF-16, because guessing
/// wrong silently misplaces every column past a non-ASCII character.
#[test]
fn anything_short_of_an_explicit_utf8_offer_falls_back_to_utf16() {
    let cases = [
        params_offering(Some(vec![PositionEncodingKind::UTF16])),
        params_offering(Some(Vec::new())),
        params_offering(None),
        InitializeParams::default(),
    ];
    for params in cases {
        assert_eq!(negotiated_encoding(&params), PositionEncoding::Utf16);
    }
}

#[test]
fn the_advertised_capabilities_mirror_the_negotiated_encoding() {
    assert_eq!(
        capabilities_for(PositionEncoding::Utf8).position_encoding,
        Some(PositionEncodingKind::UTF8)
    );
    assert_eq!(
        capabilities_for(PositionEncoding::Utf16).position_encoding,
        Some(PositionEncodingKind::UTF16)
    );
}

#[test]
fn every_feature_the_server_implements_is_advertised() {
    let capabilities = capabilities_for(PositionEncoding::Utf16);
    assert_eq!(capabilities.definition_provider, Some(OneOf::Left(true)));
    assert_eq!(capabilities.references_provider, Some(OneOf::Left(true)));
    assert_eq!(capabilities.rename_provider, Some(OneOf::Left(true)));
    assert_eq!(
        capabilities.document_symbol_provider,
        Some(OneOf::Left(true))
    );
    assert!(matches!(
        capabilities.text_document_sync,
        Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL))
    ));
}

// Exercising the deprecated field is the point: older clients still send it.
#[allow(deprecated)]
#[test]
fn a_workspace_folder_is_preferred_over_the_deprecated_root_uri() {
    let params = InitializeParams {
        workspace_folders: Some(vec![folder(file_uri("chosen"))]),
        root_uri: Some(file_uri("ignored")),
        ..InitializeParams::default()
    };
    let root = workspace_root(&params).unwrap();
    assert!(root.as_str().ends_with("chosen"), "{root}");
}

// Exercising the deprecated field is the point: older clients still send it.
#[allow(deprecated)]
#[test]
fn root_uri_is_used_when_no_folder_was_sent() {
    let params = InitializeParams {
        workspace_folders: Some(Vec::new()),
        root_uri: Some(file_uri("fallback")),
        ..InitializeParams::default()
    };
    let root = workspace_root(&params).unwrap();
    assert!(root.as_str().ends_with("fallback"), "{root}");
}

#[test]
fn a_client_that_names_no_workspace_leaves_the_root_undecided() {
    assert_eq!(workspace_root(&InitializeParams::default()), None);
}

/// Bad input and internal failure carry different JSON-RPC codes: an editor retries one and
/// surfaces the other.
#[test]
fn each_handler_failure_carries_its_own_json_rpc_code() {
    let invalid = response_for(
        RequestId::from(1),
        Err(HandlerError::InvalidParams("bad shape".to_owned())),
    );
    let error = invalid.response_result.unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidParams as i32);
    assert_eq!(error.message, "bad shape");

    let internal = response_for(
        RequestId::from(2),
        Err(HandlerError::Internal("boom".to_owned())),
    );
    let error = internal.response_result.unwrap_err();
    assert_eq!(error.code, ErrorCode::InternalError as i32);
}

#[test]
fn a_successful_handler_answers_with_its_value_under_the_request_id() {
    let response = response_for(RequestId::from(7), Ok(serde_json::json!({"ok": true})));
    assert_eq!(response.id, RequestId::from(7));
    assert_eq!(response.response_result.unwrap()["ok"], true);
}

#[test]
fn an_anyhow_error_becomes_an_internal_failure_carrying_its_context_chain() {
    let error: HandlerError = anyhow::anyhow!("root cause")
        .context("while resolving")
        .into();
    let response = response_for(RequestId::from(1), Err(error));
    let message = response.response_result.unwrap_err().message;
    assert!(message.contains("while resolving"), "{message}");
    assert!(message.contains("root cause"), "{message}");
}

#[test]
fn published_diagnostics_are_an_unversioned_notification_for_the_named_file() {
    let uri = file_uri("repo/a.md");
    let Message::Notification(notification) = publish(uri.clone(), Vec::new()) else {
        panic!("expected a notification");
    };
    assert_eq!(notification.method, PublishDiagnostics::METHOD);

    let params: PublishDiagnosticsParams = serde_json::from_value(notification.params).unwrap();
    assert_eq!(params.uri, uri);
    assert!(params.diagnostics.is_empty());
    assert_eq!(
        params.version, None,
        "anchr publishes for the file, not a revision"
    );
}

#[test]
fn a_document_symbol_carries_both_its_full_range_and_its_selection() {
    let full = Range {
        start: Position {
            line: 1,
            character: 0,
        },
        end: Position {
            line: 1,
            character: 20,
        },
    };
    let selection = Range {
        start: Position {
            line: 1,
            character: 8,
        },
        end: Position {
            line: 1,
            character: 12,
        },
    };
    let symbol = document_symbol("auth/flow", "anchor", SymbolKind::KEY, full, selection);

    assert_eq!(symbol.name, "auth/flow");
    assert_eq!(symbol.detail, Some("anchor".to_owned()));
    assert_eq!(symbol.kind, SymbolKind::KEY);
    assert_eq!(symbol.range, full);
    assert_eq!(symbol.selection_range, selection);
    assert!(symbol.children.is_none());
}
