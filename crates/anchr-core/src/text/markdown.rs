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
mod tests {
    use super::*;
    use crate::marker::lex;

    /// The marker bodies the lexer sees after region exclusion, in document order.
    fn checked_bodies(source: &str) -> Vec<&str> {
        let lexed = lex(source, &text_regions(source).unwrap()).unwrap();
        assert!(lexed.malformed.is_empty(), "{:?}", lexed.malformed);
        lexed
            .markers
            .iter()
            .map(|marker| &source[marker.body_span.start..marker.body_span.end])
            .collect()
    }

    #[test]
    fn a_parser_panic_is_an_error_not_a_crash() {
        let minimized_fuzz_input = "*\t[G]:`\n\t\t";
        assert!(matches!(
            text_regions(minimized_fuzz_input),
            Err(AnalyzeError::ParserPanicked)
        ));
        assert!(matches!(
            coverage_regions(minimized_fuzz_input),
            Err(AnalyzeError::ParserPanicked)
        ));
    }

    #[test]
    fn markers_in_headings_paragraphs_and_html_comments_are_checked() {
        let source = "# Title @anchor[title]\n\nSee @ref[#title].\n\n<!-- @anchor[hidden] -->\n";
        assert_eq!(checked_bodies(source), vec!["title", "#title", "hidden"]);
    }

    #[test]
    fn alias_uses_are_lexed_wherever_markers_are() {
        let source = "# Title @[A]\n<!-- @[B] -->\n| @[C] | `@[D]` |\n@ref[a.md as E]\n";
        assert_eq!(checked_bodies(source), vec!["A", "B", "C", "a.md as E"]);
    }

    #[test]
    fn fenced_code_blocks_are_not_checked() {
        let source = "before @ref[a.md]\n\n```\n@ref[in-fence.md]\n```\n\nafter @ref[b.md]\n";
        assert_eq!(checked_bodies(source), vec!["a.md", "b.md"]);
    }

    #[test]
    fn tilde_fences_and_longer_closing_fences_are_not_checked() {
        let source = "~~~md\n@ref[x.md]\n~~~\n\n````\n```\n@ref[y.md]\n```\n````\n@ref[ok.md]\n";
        assert_eq!(checked_bodies(source), vec!["ok.md"]);
    }

    #[test]
    fn an_unclosed_fence_excludes_everything_to_the_end_of_file() {
        let source = "@ref[a.md]\n\n```\n@ref[b.md]\n\nstill code @ref[c.md]\n";
        assert_eq!(checked_bodies(source), vec!["a.md"]);
    }

    #[test]
    fn indented_code_blocks_are_not_checked() {
        let source = "para @ref[a.md]\n\n    @ref[indented.md]\n\npara @ref[b.md]\n";
        assert_eq!(checked_bodies(source), vec!["a.md", "b.md"]);
    }

    #[test]
    fn inline_code_spans_are_not_checked() {
        let source = "use `@ref[x.md]` like @ref[y.md] or ``@ref[`z`.md]``\n";
        assert_eq!(checked_bodies(source), vec!["y.md"]);
    }

    #[test]
    fn fences_inside_blockquotes_and_list_items_are_not_checked() {
        let source = "> quoted @ref[q.md]\n> ```\n> @ref[qf.md]\n> ```\n\n- item @ref[i.md]\n\n  ```\n  @ref[if.md]\n  ```\n\n- second\n\n      @ref[indented-in-list.md]\n";
        assert_eq!(checked_bodies(source), vec!["q.md", "i.md"]);
    }

    #[test]
    fn brackets_that_look_like_reference_links_do_not_split_a_marker() {
        let source = "See @ref[#target] and @ref[docs/a.md][1] plus [text](@ref[#x]).\n\n[1]: http://example.com\n";
        assert_eq!(checked_bodies(source), vec!["#target", "docs/a.md", "#x"]);
    }

    #[test]
    fn escaped_markers_are_not_lexed() {
        let source =
            r"Write \@ref[x.md] or @ref\[x.md\] to show the syntax; @ref[real.md] is live.";
        assert_eq!(checked_bodies(source), vec!["real.md"]);
    }

    #[test]
    fn heading_attributes_and_tables_do_not_hide_markers() {
        let source = "## Section {#sec} @anchor[sec]\n\n| a | b |\n|---|---|\n| @ref[#sec] | `@ref[no.md]` |\n";
        assert_eq!(checked_bodies(source), vec!["sec", "#sec"]);
    }

    #[test]
    fn a_file_that_is_all_code_yields_no_markers() {
        assert!(checked_bodies("```\n@ref[x.md]\n```\n").is_empty());
        assert!(text_regions("").unwrap().is_empty());
    }

    #[test]
    fn coverage_regions_keep_code_spans_but_drop_links_and_blocks() {
        let source = "See `docs/a.md` and [guide](docs/guide.md) here.\n\n```\n`fenced.md`\n```\n";
        let regions: Vec<(RegionKind, &str)> = coverage_regions(source)
            .unwrap()
            .iter()
            .map(|region| (region.kind, &source[region.span.start..region.span.end]))
            .collect();
        assert_eq!(
            regions,
            vec![
                (RegionKind::Prose, "See "),
                (RegionKind::InlineCode, "`docs/a.md`"),
                (RegionKind::Prose, " and "),
                (RegionKind::Prose, " here.\n\n"),
                (RegionKind::Prose, "\n"),
            ]
        );
    }
}
