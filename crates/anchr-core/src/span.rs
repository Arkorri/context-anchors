use std::fmt;

use line_index::TextSize;

/// Half-open byte range within one file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ByteSpan {
    pub start: usize,
    pub end: usize,
}

impl ByteSpan {
    pub fn new(start: usize, end: usize) -> Self {
        debug_assert!(start <= end, "span start {start} is after end {end}");
        Self { start, end }
    }

    pub fn len(&self) -> usize {
        self.end - self.start
    }

    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }

    pub fn contains(&self, offset: usize) -> bool {
        (self.start..self.end).contains(&offset)
    }

    pub fn intersects(&self, other: ByteSpan) -> bool {
        self.start < other.end && other.start < self.end
    }

    pub fn shifted_by(&self, base: usize) -> ByteSpan {
        ByteSpan::new(self.start + base, self.end + base)
    }
}

impl From<std::ops::Range<usize>> for ByteSpan {
    fn from(range: std::ops::Range<usize>) -> Self {
        ByteSpan::new(range.start, range.end)
    }
}

/// One-based line and column for display. Column counts UTF-8 bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LineCol {
    pub line: u32,
    pub col: u32,
}

impl fmt::Display for LineCol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.line, self.col)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("byte offset {offset} exceeds the 4 GiB position limit")]
pub struct PositionOverflow {
    pub offset: usize,
}

/// Byte offset → line/column lookup for one file's text, built once per file.
#[derive(Debug, Clone)]
pub struct LineIndex {
    inner: line_index::LineIndex,
    text_len: usize,
}

impl LineIndex {
    pub fn new(text: &str) -> Result<Self, PositionOverflow> {
        TextSize::try_from(text.len()).map_err(|_| PositionOverflow { offset: text.len() })?;
        Ok(Self {
            inner: line_index::LineIndex::new(text),
            text_len: text.len(),
        })
    }

    /// `offset` may equal the text length (end of file); anything beyond is an overflow.
    pub fn line_col(&self, offset: usize) -> Result<LineCol, PositionOverflow> {
        let overflow = PositionOverflow { offset };
        if offset > self.text_len {
            return Err(overflow);
        }
        let size = TextSize::try_from(offset).map_err(|_| overflow)?;
        let zero_based = self.inner.try_line_col(size).ok_or(overflow)?;
        Ok(LineCol {
            line: zero_based.line + 1,
            col: zero_based.col + 1,
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn spans_intersect_when_they_overlap_by_at_least_one_byte() {
        let a = ByteSpan::new(0, 5);
        assert!(a.intersects(ByteSpan::new(4, 10)));
        assert!(!a.intersects(ByteSpan::new(5, 10)));
        assert!(!ByteSpan::new(5, 10).intersects(a));
    }

    #[test]
    fn line_col_is_one_based_and_counts_utf8_bytes() {
        let index = LineIndex::new("ab\né@x").unwrap();
        assert_eq!(index.line_col(0).unwrap(), LineCol { line: 1, col: 1 });
        assert_eq!(index.line_col(3).unwrap(), LineCol { line: 2, col: 1 });
        // `é` is two bytes, so `@` sits at byte column 3.
        assert_eq!(index.line_col(5).unwrap(), LineCol { line: 2, col: 3 });
    }

    #[test]
    fn end_of_text_is_a_valid_position_but_beyond_it_is_not() {
        let index = LineIndex::new("abc").unwrap();
        assert_eq!(index.line_col(3).unwrap(), LineCol { line: 1, col: 4 });
        assert_eq!(index.line_col(4), Err(PositionOverflow { offset: 4 }));
    }

    #[test]
    fn crlf_leaves_the_carriage_return_on_the_previous_line() {
        let index = LineIndex::new("a\r\nb").unwrap();
        assert_eq!(index.line_col(3).unwrap(), LineCol { line: 2, col: 1 });
    }
}
