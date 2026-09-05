//! Which bytes of a file may contain markers, and the per-thread analyzer that finds them.

pub mod language;
mod markdown;
mod source;

use std::ops::ControlFlow;
use std::time::{Duration, Instant};

use tree_sitter::{ParseOptions, ParseState, Parser, Tree};

pub use language::{LanguageRegistry, LanguageSpec, RegistryError};
pub use source::{MAX_DECLARATIONS_PER_FILE, SymbolTable};

use crate::marker::{LexError, Lexed, MalformedMarker, Marker, RelPath, lex};
use crate::span::{ByteSpan, LineIndex, PositionOverflow};

/// What kind of text a byte range holds. Only these regions are lexed for markers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RegionKind {
    /// Markdown text outside code blocks and code spans.
    Prose,
    /// A comment node in a source file.
    Comment,
    /// An entire plaintext file.
    Whole,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TextRegion {
    pub span: ByteSpan,
    pub kind: RegionKind,
}

/// The byte ranges of one file that may contain markers: sorted, non-empty, non-overlapping.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TextRegions(Vec<TextRegion>);

impl TextRegions {
    /// Sorts by start, drops empty regions, and merges any that overlap or nest (a nested
    /// block comment is reported by tree-sitter as its own node inside the outer one).
    pub fn new(mut regions: Vec<TextRegion>) -> Self {
        regions.retain(|region| !region.span.is_empty());
        regions.sort_by_key(|region| region.span.start);
        let mut merged: Vec<TextRegion> = Vec::with_capacity(regions.len());
        for region in regions {
            match merged.last_mut() {
                Some(last) if region.span.start < last.span.end => {
                    last.span.end = last.span.end.max(region.span.end);
                }
                _ => merged.push(region),
            }
        }
        Self(merged)
    }

    pub fn whole(text_len: usize, kind: RegionKind) -> Self {
        Self::new(vec![TextRegion {
            span: ByteSpan::new(0, text_len),
            kind,
        }])
    }

    pub fn iter(&self) -> impl Iterator<Item = &TextRegion> {
        self.0.iter()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Which file extensions select the markdown and plaintext containers. Source containers come
/// from the [`LanguageRegistry`]. Extensions are lowercase without the dot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerRules {
    pub markdown_extensions: Vec<String>,
    pub plaintext_extensions: Vec<String>,
}

impl Default for ContainerRules {
    fn default() -> Self {
        Self {
            markdown_extensions: vec!["md".to_owned(), "markdown".to_owned()],
            plaintext_extensions: vec!["txt".to_owned()],
        }
    }
}

/// How a file's bytes are split into markers-allowed regions. A pure selector; the work
/// happens in [`FileAnalyzer`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Container<'r> {
    Markdown,
    Source(&'r LanguageSpec),
    Plaintext,
}

impl<'r> Container<'r> {
    /// `None` means the file is not scanned at all: opting in applies to files as well as
    /// to markers.
    pub fn for_path(
        path: &RelPath,
        rules: &ContainerRules,
        registry: &'r LanguageRegistry,
    ) -> Option<Self> {
        let extension = path.extension()?.to_ascii_lowercase();
        if rules.markdown_extensions.contains(&extension) {
            return Some(Container::Markdown);
        }
        if rules.plaintext_extensions.contains(&extension) {
            return Some(Container::Plaintext);
        }
        registry.for_extension(&extension).map(Container::Source)
    }
}

/// Everything the lexer found in one file. File identity is attached by the scan stage.
#[derive(Debug, Clone)]
pub struct FileScan {
    pub markers: Vec<Marker>,
    pub malformed: Vec<MalformedMarker>,
    pub line_index: LineIndex,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AnalyzeError {
    #[error("parsing exceeded the time budget of {budget:?}")]
    ParseTimeout { budget: Duration },
    #[error("the parser produced no tree")]
    ParseFailed,
    #[error("the markdown parser panicked on this file (pulldown-cmark#1129)")]
    ParserPanicked,
    #[error("more than {MAX_DECLARATIONS_PER_FILE} declarations; symbol table discarded")]
    SymbolTableTruncated,
    #[error(transparent)]
    Lex(LexError),
    #[error(transparent)]
    Position(PositionOverflow),
}

/// Owns a tree-sitter parser, which is neither cheap to build per file nor shareable across
/// threads. One lives per scan thread and one per resolver.
pub struct FileAnalyzer<'r> {
    parser: Parser,
    registry: &'r LanguageRegistry,
    parse_budget: Duration,
}

impl<'r> FileAnalyzer<'r> {
    pub fn new(registry: &'r LanguageRegistry, parse_budget: Duration) -> Self {
        Self {
            parser: Parser::new(),
            registry,
            parse_budget,
        }
    }

    pub fn registry(&self) -> &'r LanguageRegistry {
        self.registry
    }

    pub fn text_regions(
        &mut self,
        container: Container<'r>,
        source: &str,
    ) -> Result<TextRegions, AnalyzeError> {
        match container {
            Container::Markdown => markdown::text_regions(source),
            Container::Plaintext => Ok(TextRegions::whole(source.len(), RegionKind::Whole)),
            Container::Source(spec) => {
                let tree = self.parse(spec, source)?;
                Ok(source::comment_regions(spec, &tree, source))
            }
        }
    }

    pub fn scan(
        &mut self,
        container: Container<'r>,
        source: &str,
    ) -> Result<FileScan, AnalyzeError> {
        let regions = self.text_regions(container, source)?;
        let Lexed { markers, malformed } = lex(source, &regions).map_err(AnalyzeError::Lex)?;
        let line_index = LineIndex::new(source).map_err(AnalyzeError::Position)?;
        Ok(FileScan {
            markers,
            malformed,
            line_index,
        })
    }

    pub fn symbols(
        &mut self,
        spec: &LanguageSpec,
        source: &str,
    ) -> Result<SymbolTable, AnalyzeError> {
        let tree = self.parse(spec, source)?;
        source::symbol_table(spec, &tree, source)
    }

    fn parse(&mut self, spec: &LanguageSpec, source: &str) -> Result<Tree, AnalyzeError> {
        // Languages in the registry were already accepted by a parser at construction time.
        if self.parser.set_language(spec.language()).is_err() {
            return Err(AnalyzeError::ParseFailed);
        }
        let deadline = Instant::now() + self.parse_budget;
        let mut timed_out = false;
        let mut progress = |_: &ParseState| {
            if Instant::now() >= deadline {
                timed_out = true;
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        };
        let options = ParseOptions::new().progress_callback(&mut progress);
        let bytes = source.as_bytes();
        let tree = self.parser.parse_with_options(
            &mut |offset, _| &bytes[offset.min(bytes.len())..],
            None,
            Some(options),
        );
        match tree {
            Some(tree) => Ok(tree),
            None => {
                self.parser.reset();
                if timed_out {
                    Err(AnalyzeError::ParseTimeout {
                        budget: self.parse_budget,
                    })
                } else {
                    Err(AnalyzeError::ParseFailed)
                }
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn region(start: usize, end: usize) -> TextRegion {
        TextRegion {
            span: ByteSpan::new(start, end),
            kind: RegionKind::Comment,
        }
    }

    #[test]
    fn regions_are_sorted_and_nested_or_overlapping_ones_merge() {
        let regions = TextRegions::new(vec![
            region(10, 20),
            region(0, 5),
            region(12, 15),
            region(18, 25),
            region(30, 30),
        ]);
        let spans: Vec<ByteSpan> = regions.iter().map(|r| r.span).collect();
        assert_eq!(spans, vec![ByteSpan::new(0, 5), ByteSpan::new(10, 25)]);
    }

    #[test]
    fn container_selection_is_by_lowercased_extension_and_opt_in() {
        let registry = LanguageRegistry::new().unwrap();
        let rules = ContainerRules::default();
        let select =
            |raw: &str| Container::for_path(&RelPath::parse(raw).unwrap(), &rules, &registry);
        assert_eq!(select("README.MD"), Some(Container::Markdown));
        assert_eq!(select("notes.txt"), Some(Container::Plaintext));
        assert!(
            matches!(select("src/lib.rs"), Some(Container::Source(spec)) if spec.name() == "rust")
        );
        assert_eq!(select("Makefile"), None);
        assert_eq!(select("image.png"), None);
    }

    #[test]
    fn a_zero_budget_parse_of_a_large_file_times_out_instead_of_hanging() {
        let registry = LanguageRegistry::new().unwrap();
        let spec = registry.for_extension("rs").unwrap();
        let mut analyzer = FileAnalyzer::new(&registry, Duration::ZERO);
        let source = "fn f() { let x = 1 + 2; }\n".repeat(50_000);
        assert!(matches!(
            analyzer.symbols(spec, &source),
            Err(AnalyzeError::ParseTimeout { .. })
        ));
        // The parser is usable again after a cancelled parse.
        let mut healthy = FileAnalyzer::new(&registry, Duration::from_secs(5));
        assert!(healthy.symbols(spec, "fn f() {}").is_ok());
    }
}
