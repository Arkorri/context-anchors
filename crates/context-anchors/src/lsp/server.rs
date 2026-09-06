use std::collections::{BTreeMap, HashMap};

use anchr_core::check::{CheckOptions, Workspace, check};
use anchr_core::diagnostic::{Locations, Severity};
use anchr_core::edit::TextEdit as ByteEdit;
use anchr_core::marker::{Alias, AnchorId, Marker, MarkerPayload, RefTarget};
use anchr_core::rename::{plan_alias_rename, plan_rename};
use anchr_core::resolve::IndexedRoot;
use anchr_core::root::FilePath;
use anchr_core::span::{LineIndex, PositionEncoding};
use anchr_core::text::FileAnalyzer;
use anyhow::Context;
use camino::Utf8PathBuf;
use ls_types::notification::{
    DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, Notification as _,
    PublishDiagnostics,
};
use ls_types::request::{DocumentSymbolRequest, GotoDefinition, References, Rename, Request as _};
use ls_types::{
    Diagnostic, DiagnosticRelatedInformation, DiagnosticSeverity, DidChangeTextDocumentParams,
    DidCloseTextDocumentParams, DidOpenTextDocumentParams, DocumentSymbol, DocumentSymbolParams,
    DocumentSymbolResponse, GotoDefinitionParams, GotoDefinitionResponse, InitializeParams,
    Location, NumberOrString, OneOf, Position, PositionEncodingKind, PublishDiagnosticsParams,
    Range, ReferenceParams, RenameParams, ServerCapabilities, SymbolKind,
    TextDocumentSyncCapability, TextDocumentSyncKind, TextEdit, Uri, WorkspaceEdit,
};
use lsp_server::{ErrorCode, Message, Notification, Request, Response};

use super::convert;
use crate::render::json::code;

pub struct Server {
    workspace: Workspace,
    root_dir: Utf8PathBuf,
    encoding: PositionEncoding,
    open: HashMap<FilePath, Uri>,
}

enum HandlerError {
    InvalidParams(String),
    Internal(String),
}

impl From<anyhow::Error> for HandlerError {
    fn from(error: anyhow::Error) -> Self {
        HandlerError::Internal(format!("{error:#}"))
    }
}

type HandlerResult = Result<serde_json::Value, HandlerError>;

impl Server {
    pub fn new(params: &InitializeParams) -> anyhow::Result<Self> {
        let start = workspace_root(params).unwrap_or(crate::commands::current_dir()?);
        let discovered = anchr_core::config::discover(&start)?;
        let root_dir = discovered.root_dir.clone();
        let workspace = Workspace::load(discovered)?;
        let offers_utf8 = params
            .capabilities
            .general
            .as_ref()
            .and_then(|general| general.position_encodings.as_ref())
            .is_some_and(|encodings| encodings.contains(&PositionEncodingKind::UTF8));
        Ok(Self {
            workspace,
            root_dir,
            encoding: if offers_utf8 {
                PositionEncoding::Utf8
            } else {
                PositionEncoding::Utf16
            },
            open: HashMap::new(),
        })
    }

    pub fn capabilities(&self) -> ServerCapabilities {
        ServerCapabilities {
            position_encoding: Some(match self.encoding {
                PositionEncoding::Utf8 => PositionEncodingKind::UTF8,
                PositionEncoding::Utf16 => PositionEncodingKind::UTF16,
            }),
            text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
            definition_provider: Some(OneOf::Left(true)),
            references_provider: Some(OneOf::Left(true)),
            rename_provider: Some(OneOf::Left(true)),
            document_symbol_provider: Some(OneOf::Left(true)),
            ..ServerCapabilities::default()
        }
    }

    pub fn handle_request(&mut self, request: Request) -> Response {
        let id = request.id.clone();
        let outcome = match request.method.as_str() {
            GotoDefinition::METHOD => {
                self.with_params(request, |server, params| server.definition(params))
            }
            References::METHOD => {
                self.with_params(request, |server, params| server.references(params))
            }
            Rename::METHOD => self.with_params(request, |server, params| server.rename(params)),
            DocumentSymbolRequest::METHOD => {
                self.with_params(request, |server, params| server.document_symbols(&params))
            }
            other => Err(HandlerError::InvalidParams(format!(
                "unsupported request `{other}`"
            ))),
        };
        match outcome {
            Ok(value) => Response::new_ok(id, value),
            Err(HandlerError::InvalidParams(message)) => {
                Response::new_err(id, ErrorCode::InvalidParams as i32, message)
            }
            Err(HandlerError::Internal(message)) => {
                Response::new_err(id, ErrorCode::InternalError as i32, message)
            }
        }
    }

    /// Notifications produce outgoing messages (published diagnostics) rather than a response.
    pub fn handle_notification(&mut self, notification: Notification) -> Vec<Message> {
        let outcome = match notification.method.as_str() {
            DidOpenTextDocument::METHOD => {
                serde_json::from_value::<DidOpenTextDocumentParams>(notification.params)
                    .ok()
                    .map(|params| self.did_open(params))
            }
            DidChangeTextDocument::METHOD => {
                serde_json::from_value::<DidChangeTextDocumentParams>(notification.params)
                    .ok()
                    .map(|params| self.did_change(params))
            }
            DidCloseTextDocument::METHOD => {
                serde_json::from_value::<DidCloseTextDocumentParams>(notification.params)
                    .ok()
                    .map(|params| self.did_close(params))
            }
            _ => None,
        };
        outcome.unwrap_or_default()
    }

    fn with_params<P: serde::de::DeserializeOwned>(
        &mut self,
        request: Request,
        handler: impl FnOnce(&mut Self, P) -> HandlerResult,
    ) -> HandlerResult {
        let params: P = serde_json::from_value(request.params)
            .map_err(|error| HandlerError::InvalidParams(error.to_string()))?;
        handler(self, params)
    }

    fn did_open(&mut self, params: DidOpenTextDocumentParams) -> Vec<Message> {
        let Some(path) = convert::file_path_in(&self.root_dir, &params.text_document.uri) else {
            return Vec::new();
        };
        self.open.insert(path.clone(), params.text_document.uri);
        self.replace_text(path, &params.text_document.text)
    }

    fn did_change(&mut self, params: DidChangeTextDocumentParams) -> Vec<Message> {
        let Some(path) = convert::file_path_in(&self.root_dir, &params.text_document.uri) else {
            return Vec::new();
        };
        // Full sync: the last change carries the whole document.
        let Some(change) = params.content_changes.last() else {
            return Vec::new();
        };
        self.open
            .entry(path.clone())
            .or_insert(params.text_document.uri);
        self.replace_text(path, &change.text)
    }

    fn did_close(&mut self, params: DidCloseTextDocumentParams) -> Vec<Message> {
        let Some(path) = convert::file_path_in(&self.root_dir, &params.text_document.uri) else {
            return Vec::new();
        };
        self.open.remove(&path);
        if self.workspace.reload_file(path).is_err() {
            return Vec::new();
        }
        let mut messages = self.publish_all();
        messages.push(publish(params.text_document.uri, Vec::new()));
        messages
    }

    fn replace_text(&mut self, path: FilePath, text: &str) -> Vec<Message> {
        match self.workspace.update_file(path, text) {
            Ok(_) => self.publish_all(),
            Err(error) => {
                eprintln!("anchr lsp: could not analyze document: {error}");
                Vec::new()
            }
        }
    }

    /// One document's anchors can change every other document's diagnostics, so all open
    /// documents are re-published after any change.
    fn publish_all(&self) -> Vec<Message> {
        let mut messages = Vec::with_capacity(self.open.len());
        for (path, uri) in &self.open {
            messages.push(publish(uri.clone(), self.diagnostics_for(path)));
        }
        messages
    }

    fn diagnostics_for(&self, path: &FilePath) -> Vec<Diagnostic> {
        let options = CheckOptions {
            unverified: None,
            only_files: vec![path.clone()],
        };
        let Ok(report) = check(&self.workspace, &options) else {
            return Vec::new();
        };
        let (current_root, index) = self.workspace.current();
        let (Some(record), Some(uri)) = (
            index.file_record(path),
            convert::uri_for(&current_root.dir, path),
        ) else {
            return Vec::new();
        };
        let line_index = &record.line_index;
        let mut diagnostics = Vec::new();
        for diagnostic in &report.diagnostics {
            let severity = match diagnostic.severity {
                Severity::Error => DiagnosticSeverity::ERROR,
                Severity::Unverified => DiagnosticSeverity::WARNING,
            };
            let mut message = diagnostic.kind.to_string();
            if let Some(suggestion) = &diagnostic.suggestion {
                message.push_str(&format!("; did you mean `{suggestion}`?"));
            }
            for note in &diagnostic.notes {
                message.push_str(&format!("; {note}"));
            }
            let sites: Vec<(Range, Option<Vec<DiagnosticRelatedInformation>>)> = match &diagnostic
                .locations
            {
                Locations::Sites(sites) => sites
                    .iter()
                    .filter(|located| located.site.path == *path)
                    .filter_map(|located| {
                        let range = convert::range(line_index, located.site.span, self.encoding)?;
                        let related =
                            self.alias_uses_of_declaration_at(record, &uri, located.site.span);
                        Some((range, related))
                    })
                    .collect(),
                Locations::Files(files) => files
                    .iter()
                    .filter(|file| file.path == *path)
                    .map(|_| (Range::default(), None))
                    .collect(),
                Locations::Roots(_) => Vec::new(),
            };
            for (range, related_information) in sites {
                diagnostics.push(Diagnostic {
                    range,
                    severity: Some(severity),
                    code: Some(NumberOrString::String(code(&diagnostic.kind).to_owned())),
                    code_description: None,
                    source: Some("anchr".to_owned()),
                    message: message.clone(),
                    related_information,
                    tags: None,
                    data: None,
                });
            }
        }
        diagnostics
    }

    /// When the diagnosed marker declares an alias, its uses in the same file, so an editor can
    /// show what a broken declaration affects without the uses becoming errors themselves.
    fn alias_uses_of_declaration_at(
        &self,
        record: &anchr_core::index::FileRecord,
        uri: &Uri,
        span: anchr_core::span::ByteSpan,
    ) -> Option<Vec<DiagnosticRelatedInformation>> {
        let declared = record
            .markers
            .iter()
            .find_map(|marker| match &marker.payload {
                MarkerPayload::Ref {
                    alias: Some(declared),
                    ..
                } if marker.span == span => Some(&declared.alias),
                MarkerPayload::Anchor { .. }
                | MarkerPayload::Ref { .. }
                | MarkerPayload::Use { .. } => None,
            })?;
        let related: Vec<DiagnosticRelatedInformation> = record
            .markers
            .iter()
            .filter(|marker| {
                matches!(&marker.payload, MarkerPayload::Use { alias } if alias == declared)
            })
            .filter_map(|marker| {
                let range = convert::range(&record.line_index, marker.span, self.encoding)?;
                Some(DiagnosticRelatedInformation {
                    location: Location {
                        uri: uri.clone(),
                        range,
                    },
                    message: format!("use of alias `{declared}`"),
                })
            })
            .collect();
        (!related.is_empty()).then_some(related)
    }

    fn definition(&mut self, params: GotoDefinitionParams) -> HandlerResult {
        let position = params.text_document_position_params;
        let Some((path, marker, _)) =
            self.marker_at(&position.text_document.uri, position.position)
        else {
            return Ok(serde_json::Value::Null);
        };
        let locations = match &marker.payload {
            MarkerPayload::Ref { target, .. } => self.definition_locations(target)?,
            MarkerPayload::Use { alias } => {
                let (current_root, index) = self.workspace.current();
                let Some(binding) = index
                    .file_record(&path)
                    .and_then(|record| record.aliases.binding(alias))
                else {
                    return Ok(serde_json::Value::Null);
                };
                let mut locations = self.definition_locations(&binding.target)?;
                if let (Some(line_index), Some(uri)) = (
                    index.line_index(&path),
                    convert::uri_for(&current_root.dir, &path),
                ) && let Some(range) =
                    convert::range(line_index, binding.declaration, self.encoding)
                {
                    locations.push(Location { uri, range });
                }
                locations
            }
            MarkerPayload::Anchor { .. } => return Ok(serde_json::Value::Null),
        };
        serde_json::to_value(GotoDefinitionResponse::Array(locations))
            .map_err(|error| HandlerError::Internal(error.to_string()))
    }

    fn definition_locations(&self, target: &RefTarget) -> Result<Vec<Location>, HandlerError> {
        let (current_root, _) = self.workspace.current();
        let root_name = target.root().unwrap_or(&current_root.name).clone();
        let Some(IndexedRoot::Present { root, index }) = self.workspace.roots.get(&root_name)
        else {
            return Ok(Vec::new());
        };
        match target {
            RefTarget::Anchor { id, .. } => Ok(index
                .anchor_sites(id)
                .iter()
                .filter_map(|site| {
                    let line_index = index.line_index(&site.path)?;
                    let uri = convert::uri_for(&root.dir, &site.path)?;
                    let range = convert::range(line_index, site.span, self.encoding)?;
                    Some(Location { uri, range })
                })
                .collect()),
            RefTarget::Path { path, .. } => {
                let file = FilePath::from(path);
                Ok(convert::uri_for(&root.dir, &file)
                    .map(|uri| Location {
                        uri,
                        range: Range::default(),
                    })
                    .into_iter()
                    .collect())
            }
            RefTarget::Symbol { path, name, .. } => {
                let file = FilePath::from(path);
                let absolute = root.dir.join(path.as_path());
                let Ok(source) = std::fs::read_to_string(&absolute) else {
                    return Ok(Vec::new());
                };
                let Some(spec) = path.extension().and_then(|ext| {
                    self.workspace
                        .registry
                        .for_extension(&ext.to_ascii_lowercase())
                }) else {
                    return Ok(Vec::new());
                };
                let mut analyzer =
                    FileAnalyzer::new(&self.workspace.registry, root.config.scan.parse_budget);
                let table = analyzer
                    .symbols(spec, &source)
                    .context("analyzing the target file")?;
                let line_index = LineIndex::new(&source).context("indexing the target file")?;
                let Some(uri) = convert::uri_for(&root.dir, &file) else {
                    return Ok(Vec::new());
                };
                Ok(table
                    .spans(name)
                    .iter()
                    .filter_map(|span| convert::range(&line_index, *span, self.encoding))
                    .map(|range| Location {
                        uri: uri.clone(),
                        range,
                    })
                    .collect())
            }
        }
    }

    fn references(&mut self, params: ReferenceParams) -> HandlerResult {
        let position = params.text_document_position;
        let Some((path, marker, _)) =
            self.marker_at(&position.text_document.uri, position.position)
        else {
            return Ok(serde_json::Value::Null);
        };
        let (current_root, index) = self.workspace.current();
        let target = match &marker.payload {
            MarkerPayload::Anchor { id } => RefTarget::Anchor {
                root: Some(current_root.name.clone()),
                id: id.clone(),
            },
            MarkerPayload::Ref { target, .. } => target.clone(),
            MarkerPayload::Use { alias } => {
                let Some(binding) = index
                    .file_record(&path)
                    .and_then(|record| record.aliases.binding(alias))
                else {
                    return Ok(serde_json::Value::Null);
                };
                binding.target.clone()
            }
        };
        let mut locations: Vec<Location> = index
            .backrefs(&target)
            .filter_map(|reference| {
                let line_index = index.line_index(&reference.site.path)?;
                let uri = convert::uri_for(&current_root.dir, &reference.site.path)?;
                let range = convert::range(line_index, reference.site.span, self.encoding)?;
                Some(Location { uri, range })
            })
            .collect();
        if params.context.include_declaration {
            locations.extend(self.definition_locations(&target)?);
        }
        locations.sort_by(|a, b| {
            a.uri
                .as_str()
                .cmp(b.uri.as_str())
                .then(a.range.start.line.cmp(&b.range.start.line))
                .then(a.range.start.character.cmp(&b.range.start.character))
        });
        serde_json::to_value(locations).map_err(|error| HandlerError::Internal(error.to_string()))
    }

    fn rename(&mut self, params: RenameParams) -> HandlerResult {
        let position = params.text_document_position;
        let Some((path, marker, offset)) =
            self.marker_at(&position.text_document.uri, position.position)
        else {
            return Err(HandlerError::InvalidParams(
                "no anchor, reference, or alias at this position".to_owned(),
            ));
        };
        let (current_root, _) = self.workspace.current();
        let on_alias_token = |declared: &anchr_core::marker::DeclaredAlias| {
            declared.span.contains(offset) || declared.span.end == offset
        };
        let edits = match &marker.payload {
            MarkerPayload::Use { alias } => self.plan_alias(&path, alias, &params.new_name)?,
            MarkerPayload::Ref {
                alias: Some(declared),
                ..
            } if on_alias_token(declared) => {
                self.plan_alias(&path, &declared.alias, &params.new_name)?
            }
            MarkerPayload::Anchor { id } => self.plan_anchor(id, &params.new_name)?,
            MarkerPayload::Ref {
                target: RefTarget::Anchor { root, id },
                ..
            } if root.as_ref().is_none_or(|r| *r == current_root.name) => {
                self.plan_anchor(id, &params.new_name)?
            }
            MarkerPayload::Ref { .. } => {
                return Err(HandlerError::InvalidParams(
                    "only anchors in the current root and aliases can be renamed".to_owned(),
                ));
            }
        };
        let edit = WorkspaceEdit {
            changes: Some(self.workspace_changes(&edits)),
            document_changes: None,
            change_annotations: None,
        };
        serde_json::to_value(edit).map_err(|error| HandlerError::Internal(error.to_string()))
    }

    fn plan_anchor(
        &self,
        old: &AnchorId,
        new_name: &str,
    ) -> Result<BTreeMap<FilePath, Vec<ByteEdit>>, HandlerError> {
        let new = AnchorId::parse(new_name)
            .map_err(|error| HandlerError::InvalidParams(format!("`{new_name}`: {error}")))?;
        let plan = plan_rename(&self.workspace, old, &new)
            .map_err(|error| HandlerError::InvalidParams(error.to_string()))?;
        Ok(plan.edits)
    }

    fn plan_alias(
        &self,
        path: &FilePath,
        old: &Alias,
        new_name: &str,
    ) -> Result<BTreeMap<FilePath, Vec<ByteEdit>>, HandlerError> {
        let new = Alias::parse(new_name)
            .map_err(|error| HandlerError::InvalidParams(format!("`{new_name}`: {error}")))?;
        let plan = plan_alias_rename(&self.workspace, path, old, &new)
            .map_err(|error| HandlerError::InvalidParams(error.to_string()))?;
        Ok(BTreeMap::from([(plan.path, plan.edits)]))
    }

    fn workspace_changes(
        &self,
        edits: &BTreeMap<FilePath, Vec<ByteEdit>>,
    ) -> HashMap<Uri, Vec<TextEdit>> {
        let (root, index) = self.workspace.current();
        let mut changes: HashMap<Uri, Vec<TextEdit>> = HashMap::new();
        for (path, edits) in edits {
            let Some(line_index) = index.line_index(path) else {
                continue;
            };
            let Some(uri) = convert::uri_for(&root.dir, path) else {
                continue;
            };
            let text_edits = edits
                .iter()
                .filter_map(|edit| {
                    Some(TextEdit {
                        range: convert::range(line_index, edit.span, self.encoding)?,
                        new_text: edit.replacement.clone(),
                    })
                })
                .collect();
            changes.insert(uri, text_edits);
        }
        changes
    }

    fn document_symbols(&mut self, params: &DocumentSymbolParams) -> HandlerResult {
        let Some(path) = convert::file_path_in(&self.root_dir, &params.text_document.uri) else {
            return Ok(serde_json::Value::Null);
        };
        let (_, index) = self.workspace.current();
        let (Some(record), Some(line_index)) = (index.file_record(&path), index.line_index(&path))
        else {
            return Ok(serde_json::Value::Null);
        };
        let symbols: Vec<DocumentSymbol> = record
            .markers
            .iter()
            .filter_map(|marker| match &marker.payload {
                MarkerPayload::Anchor { id } => Some(document_symbol(
                    id.as_str(),
                    "@anchor",
                    SymbolKind::KEY,
                    convert::range(line_index, marker.span, self.encoding)?,
                    convert::range(line_index, marker.body_span, self.encoding)?,
                )),
                MarkerPayload::Ref {
                    alias: Some(declared),
                    ..
                } => Some(document_symbol(
                    declared.alias.as_str(),
                    "alias",
                    SymbolKind::VARIABLE,
                    convert::range(line_index, marker.span, self.encoding)?,
                    convert::range(line_index, declared.span, self.encoding)?,
                )),
                MarkerPayload::Ref { .. } | MarkerPayload::Use { .. } => None,
            })
            .collect();
        serde_json::to_value(DocumentSymbolResponse::Nested(symbols))
            .map_err(|error| HandlerError::Internal(error.to_string()))
    }

    /// The marker under the cursor in an indexed document of the current root, with the
    /// cursor's byte offset so callers can tell which part of the marker it is on.
    fn marker_at(&self, uri: &Uri, position: Position) -> Option<(FilePath, Marker, usize)> {
        let path = convert::file_path_in(&self.root_dir, uri)?;
        let (_, index) = self.workspace.current();
        let record = index.file_record(&path)?;
        let offset = convert::offset(&record.line_index, position, self.encoding)?;
        let marker = record
            .markers
            .iter()
            .find(|marker| marker.span.contains(offset) || marker.span.end == offset)?;
        Some((path, marker.clone(), offset))
    }
}

// `root_uri` is deprecated in the protocol but still what older clients send.
#[allow(deprecated)]
fn workspace_root(params: &InitializeParams) -> Option<Utf8PathBuf> {
    let uri = params
        .workspace_folders
        .as_ref()
        .and_then(|folders| folders.first())
        .map(|folder| &folder.uri)
        .or(params.root_uri.as_ref())?;
    let path = uri.to_file_path()?;
    Utf8PathBuf::from_path_buf(path.into_owned()).ok()
}

fn publish(uri: Uri, diagnostics: Vec<Diagnostic>) -> Message {
    Message::Notification(Notification::new(
        PublishDiagnostics::METHOD.to_owned(),
        PublishDiagnosticsParams {
            uri,
            diagnostics,
            version: None,
        },
    ))
}

// `deprecated` is a required field of the protocol type even though the spec deprecates it.
#[allow(deprecated)]
fn document_symbol(
    name: &str,
    detail: &str,
    kind: SymbolKind,
    range: Range,
    selection_range: Range,
) -> DocumentSymbol {
    DocumentSymbol {
        name: name.to_owned(),
        detail: Some(detail.to_owned()),
        kind,
        tags: None,
        deprecated: None,
        range,
        selection_range,
        children: None,
    }
}

impl std::fmt::Debug for Server {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Server")
            .field("root_dir", &self.root_dir)
            .field("encoding", &self.encoding)
            .field("open", &self.open.keys().collect::<Vec<_>>())
            .finish_non_exhaustive()
    }
}
