//! Durable, checkable references for unstructured prose.
//!
//! `@anchor[id]` declares an identity at a location; `@ref[target]` asserts that a path,
//! a declaration in a source file, or an anchor resolves. This crate scans roots, lexes
//! markers out of prose and code comments, indexes anchors, resolves references, and groups
//! the results into diagnostics. The binary and the LSP server are thin adapters over it.

#[path = "check/check.rs"]
pub mod check;
#[path = "config/config.rs"]
pub mod config;
#[path = "coverage/coverage.rs"]
pub mod coverage;
#[path = "diagnostic/diagnostic.rs"]
pub mod diagnostic;
#[path = "edit/edit.rs"]
pub mod edit;
#[path = "index/index.rs"]
pub mod index;
#[path = "marker/marker.rs"]
pub mod marker;
#[path = "noref/noref.rs"]
pub mod noref;
#[path = "rename/rename.rs"]
pub mod rename;
#[path = "resolve/resolve.rs"]
pub mod resolve;
#[path = "root/root.rs"]
pub mod root;
#[path = "scan/scan.rs"]
pub mod scan;
#[path = "span/span.rs"]
pub mod span;
#[path = "suggest/suggest.rs"]
pub mod suggest;
#[path = "text/text.rs"]
pub mod text;
#[path = "tree/tree.rs"]
pub mod tree;

#[cfg(test)]
mod lib_tests;
