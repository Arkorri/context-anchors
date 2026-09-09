use std::collections::HashMap;

use tree_sitter::{QueryCursor, StreamingIterator, Tree};

use super::{AnalyzeError, LanguageSpec, RegionKind, TextRegion, TextRegions};
use crate::marker::SymbolName;
use crate::span::ByteSpan;

pub const MAX_DECLARATIONS_PER_FILE: usize = 100_000;

/// The declarations found in one source file, by name, with the span of each name.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SymbolTable {
    declarations: HashMap<String, Vec<ByteSpan>>,
    /// The tree contained ERROR nodes, so a declaration may have been invisible to the query.
    pub has_parse_errors: bool,
}

impl SymbolTable {
    pub fn contains(&self, name: &SymbolName) -> bool {
        self.declarations.contains_key(name.as_str())
    }

    pub fn spans(&self, name: &SymbolName) -> &[ByteSpan] {
        self.declarations
            .get(name.as_str())
            .map_or(&[], Vec::as_slice)
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.declarations.keys().map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.declarations.len()
    }

    pub fn is_empty(&self) -> bool {
        self.declarations.is_empty()
    }
}

/// Every comment node, backtick spans included; what the coverage scanner looks at.
pub(crate) fn raw_comment_regions(spec: &LanguageSpec, tree: &Tree, source: &str) -> TextRegions {
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(spec.comment_query(), tree.root_node(), source.as_bytes());
    let mut regions = Vec::new();
    while let Some(found) = matches.next() {
        for capture in found.captures() {
            regions.push(TextRegion {
                span: ByteSpan::from(capture.node.byte_range()),
                kind: RegionKind::Comment,
            });
        }
    }
    TextRegions::new(regions)
}

/// Comment nodes, minus backtick-delimited spans inside them: doc comments are markdown by
/// convention, and `` `@ref[x]` `` in one is an example, not a reference.
pub(crate) fn comment_regions(spec: &LanguageSpec, tree: &Tree, source: &str) -> TextRegions {
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(spec.comment_query(), tree.root_node(), source.as_bytes());
    let mut regions = Vec::new();
    while let Some(found) = matches.next() {
        for capture in found.captures() {
            let span = ByteSpan::from(capture.node.byte_range());
            if let Some(text) = source.get(span.start..span.end) {
                regions.extend(outside_inline_code(text).map(|piece| TextRegion {
                    span: piece.shifted_by(span.start),
                    kind: RegionKind::Comment,
                }));
            }
        }
    }
    TextRegions::new(regions)
}

/// The parts of `text` not inside a same-line span delimited by equal runs of backticks.
fn outside_inline_code(text: &str) -> impl Iterator<Item = ByteSpan> + '_ {
    let mut pieces = Vec::new();
    let mut cursor = 0;
    for line in text.split_inclusive('\n') {
        let line_start = cursor;
        cursor += line.len();
        let mut rest = 0;
        while let Some(open_at) = line[rest..].find('`') {
            let open = rest + open_at;
            let run = line[open..].bytes().take_while(|b| *b == b'`').count();
            let fence = &line[open..open + run];
            match line[open + run..].find(fence) {
                Some(close_at) => {
                    let close = open + run + close_at + run;
                    pieces.push(ByteSpan::new(line_start + rest, line_start + open));
                    rest = close;
                }
                None => {
                    rest = open + run;
                }
            }
        }
        pieces.push(ByteSpan::new(line_start + rest, line_start + line.len()));
    }
    pieces.into_iter().filter(|piece| !piece.is_empty())
}

/// Runs the declaration query and keeps the `@name` of every match that also carries a
/// `@definition.*` capture; `@reference.*` matches from `tags.scm` are dropped here.
pub(crate) fn symbol_table(
    spec: &LanguageSpec,
    tree: &Tree,
    source: &str,
) -> Result<SymbolTable, AnalyzeError> {
    let mut table = SymbolTable {
        declarations: HashMap::new(),
        has_parse_errors: tree.root_node().has_error(),
    };
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(
        spec.declaration_query(),
        tree.root_node(),
        source.as_bytes(),
    );
    let mut recorded = 0usize;
    while let Some(found) = matches.next() {
        let captures = found.captures();
        if !captures
            .iter()
            .any(|capture| spec.is_definition_capture(capture.index))
        {
            continue;
        }
        for capture in captures
            .iter()
            .filter(|c| c.index == spec.name_capture_index())
        {
            let range = capture.node.byte_range();
            let Some(name) = source.get(range.clone()) else {
                continue;
            };
            recorded += 1;
            if recorded > MAX_DECLARATIONS_PER_FILE {
                return Err(AnalyzeError::SymbolTableTruncated);
            }
            table
                .declarations
                .entry(name.to_owned())
                .or_default()
                .push(ByteSpan::from(range));
        }
    }
    Ok(table)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;
