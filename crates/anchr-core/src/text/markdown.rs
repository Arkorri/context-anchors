use std::panic::catch_unwind;

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

use super::{AnalyzeError, RegionKind, TextRegion, TextRegions};
use crate::span::ByteSpan;

const OPTIONS: Options = Options::ENABLE_TABLES
    .union(Options::ENABLE_FOOTNOTES)
    .union(Options::ENABLE_STRIKETHROUGH)
    .union(Options::ENABLE_TASKLISTS)
    .union(Options::ENABLE_HEADING_ATTRIBUTES);

/// The byte ranges of a markdown document that are not prose.
struct Structure {
    code_blocks: Vec<ByteSpan>,
    code_spans: Vec<ByteSpan>,
    links: Vec<ByteSpan>,
}

/// Everything except code blocks and code spans. Built as the complement of the excluded
/// ranges rather than from `Text` events, because pulldown splits text at escapes, entities,
/// and bracket characters, and a marker must never be lost to a split.
pub(crate) fn text_regions(source: &str) -> Result<TextRegions, AnalyzeError> {
    let structure = structure(source)?;
    let mut excluded = structure.code_blocks;
    excluded.extend(structure.code_spans);
    Ok(TextRegions::new(complement(
        source.len(),
        excluded,
        RegionKind::Prose,
    )))
}

/// Prose plus code spans (as `InlineCode`), minus code blocks and links.
pub(crate) fn coverage_regions(source: &str) -> Result<TextRegions, AnalyzeError> {
    let structure = structure(source)?;
    let mut excluded = structure.code_blocks;
    excluded.extend(structure.links.iter().copied());
    excluded.extend(structure.code_spans.iter().copied());
    let mut regions = complement(source.len(), excluded, RegionKind::Prose);
    let links = structure.links;
    regions.extend(
        structure
            .code_spans
            .into_iter()
            .filter(|span| !links.iter().any(|link| link.intersects(*span)))
            .map(|span| TextRegion {
                span,
                kind: RegionKind::InlineCode,
            }),
    );
    Ok(TextRegions::new(regions))
}

/// pulldown-cmark's offset iterator panics on some malformed documents
/// (pulldown-cmark/pulldown-cmark#1129); a hostile file must not take the whole check down.
fn structure(source: &str) -> Result<Structure, AnalyzeError> {
    catch_unwind(|| collect_structure(source)).map_err(|_| AnalyzeError::ParserPanicked)
}

fn collect_structure(source: &str) -> Structure {
    let mut structure = Structure {
        code_blocks: Vec::new(),
        code_spans: Vec::new(),
        links: Vec::new(),
    };
    let mut open_code_block_start: Option<usize> = None;

    for (event, range) in Parser::new_ext(source, OPTIONS).into_offset_iter() {
        match event {
            Event::Start(Tag::CodeBlock(_)) => {
                open_code_block_start.get_or_insert(range.start);
            }
            Event::End(TagEnd::CodeBlock) => {
                if let Some(start) = open_code_block_start.take() {
                    structure.code_blocks.push(ByteSpan::new(start, range.end));
                }
            }
            Event::Code(_) => structure.code_spans.push(ByteSpan::from(range)),
            Event::Start(Tag::Link { .. } | Tag::Image { .. }) => {
                structure.links.push(ByteSpan::from(range));
            }
            _ => {}
        }
    }
    if let Some(start) = open_code_block_start {
        structure
            .code_blocks
            .push(ByteSpan::new(start, source.len()));
    }
    structure
}

fn complement(len: usize, mut excluded: Vec<ByteSpan>, kind: RegionKind) -> Vec<TextRegion> {
    excluded.sort_by_key(|span| span.start);
    let mut included = Vec::with_capacity(excluded.len() + 1);
    let mut cursor = 0;
    for span in excluded {
        if span.start > cursor {
            included.push(TextRegion {
                span: ByteSpan::new(cursor, span.start),
                kind,
            });
        }
        cursor = cursor.max(span.end);
    }
    if cursor < len {
        included.push(TextRegion {
            span: ByteSpan::new(cursor, len),
            kind,
        });
    }
    included
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;
