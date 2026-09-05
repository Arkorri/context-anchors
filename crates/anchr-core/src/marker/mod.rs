//! The marker language: `@anchor[id]`, `@ref[target]` (optionally `as Alias`), and `@[Alias]`,
//! and the types they parse into.

mod alias;
mod id;
mod lex;
mod path;
mod symbol;
mod target;

use std::fmt;

pub use alias::{Alias, AliasError, MAX_ALIAS_BYTES};
pub use id::{AnchorId, IdError, MAX_ID_BYTES, MAX_SEGMENT_BYTES};
pub use lex::{LexError, Lexed, lex};
pub use path::{MAX_PATH_BYTES, PathError, PathExpectation, RelPath};
pub use symbol::{MAX_SYMBOL_BYTES, SymbolError, SymbolName};
pub use target::{ParsedTarget, RefTarget, TargetError, parse_target};

use crate::span::ByteSpan;
use crate::text::RegionKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MarkerKind {
    Anchor,
    Ref,
    /// `@[Alias]`: a use of an alias declared elsewhere in the same file.
    Use,
}

impl fmt::Display for MarkerKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            MarkerKind::Anchor => "@anchor",
            MarkerKind::Ref => "@ref",
            MarkerKind::Use => "@[...]",
        })
    }
}

/// A well-formed marker found in one file. File identity lives on the containing scan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Marker {
    pub payload: MarkerPayload,
    /// The whole marker, `@` through `]`.
    pub span: ByteSpan,
    /// The text between the brackets.
    pub body_span: ByteSpan,
    pub region: RegionKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MarkerPayload {
    Anchor {
        id: AnchorId,
    },
    Ref {
        target: RefTarget,
        /// For `#id` and `root:#id` targets, the bytes a rename rewrites.
        id_span: Option<ByteSpan>,
        /// Present when the reference was written `target as Alias`.
        alias: Option<DeclaredAlias>,
    },
    Use {
        alias: Alias,
    },
}

/// The `as Alias` clause of a declaration, with the alias token's span.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclaredAlias {
    pub alias: Alias,
    pub span: ByteSpan,
}

impl DeclaredAlias {
    pub fn shifted_by(self, offset: usize) -> Self {
        Self {
            alias: self.alias,
            span: self.span.shifted_by(offset),
        }
    }
}

impl Marker {
    pub fn kind(&self) -> MarkerKind {
        match self.payload {
            MarkerPayload::Anchor { .. } => MarkerKind::Anchor,
            MarkerPayload::Ref { .. } => MarkerKind::Ref,
            MarkerPayload::Use { .. } => MarkerKind::Use,
        }
    }
}

/// A marker that was opened but could not be parsed. Always an error: the author opted in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MalformedMarker {
    pub kind: MarkerKind,
    pub reason: MalformedReason,
    pub span: ByteSpan,
    pub region: RegionKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, thiserror::Error)]
pub enum MalformedReason {
    #[error("missing closing `]` on the same line")]
    Unclosed,
    #[error("empty body")]
    EmptyBody,
    #[error("invalid anchor id `{raw}`: {reason}")]
    InvalidAnchorId { raw: String, reason: IdError },
    #[error("invalid target `{raw}`: {reason}")]
    InvalidTarget { raw: String, reason: TargetError },
    #[error("invalid alias `{raw}`: {reason}")]
    InvalidAlias { raw: String, reason: AliasError },
}
