//! Durable, checkable references for unstructured prose.
//!
//! `@anchor[id]` declares an identity at a location; `@ref[target]` asserts that a path,
//! a declaration in a source file, or an anchor resolves. This crate scans roots, lexes
//! markers out of prose and code comments, indexes anchors, resolves references, and groups
//! the results into diagnostics. The binary and the LSP server are thin adapters over it.

pub mod config;
pub mod index;
pub mod marker;
pub mod root;
pub mod scan;
pub mod span;
pub mod text;
