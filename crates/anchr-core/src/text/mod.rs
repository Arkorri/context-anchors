use crate::span::ByteSpan;

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

/// The byte ranges of one file that may contain markers, sorted and non-overlapping.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TextRegions(Vec<TextRegion>);

impl TextRegions {
    /// Sorts by start and drops empty regions. Callers guarantee non-overlap.
    pub fn new(mut regions: Vec<TextRegion>) -> Self {
        regions.retain(|region| !region.span.is_empty());
        regions.sort_by_key(|region| region.span.start);
        Self(regions)
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
