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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, thiserror::Error)]
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

    /// Zero-based line and character in the given encoding, as the Language Server Protocol
    /// counts them.
    pub fn protocol_position(
        &self,
        offset: usize,
        encoding: PositionEncoding,
    ) -> Result<ProtocolPosition, PositionOverflow> {
        let overflow = PositionOverflow { offset };
        if offset > self.text_len {
            return Err(overflow);
        }
        let size = TextSize::try_from(offset).map_err(|_| overflow)?;
        let utf8 = self.inner.try_line_col(size).ok_or(overflow)?;
        let character = match encoding {
            PositionEncoding::Utf8 => utf8.col,
            PositionEncoding::Utf16 => {
                self.inner
                    .to_wide(line_index::WideEncoding::Utf16, utf8)
                    .ok_or(overflow)?
                    .col
            }
        };
        Ok(ProtocolPosition {
            line: utf8.line,
            character,
        })
    }

    /// The byte offset of a protocol position, or `None` when it lies outside the text.
    pub fn offset_of(
        &self,
        position: ProtocolPosition,
        encoding: PositionEncoding,
    ) -> Option<usize> {
        let utf8 = match encoding {
            PositionEncoding::Utf8 => line_index::LineCol {
                line: position.line,
                col: position.character,
            },
            PositionEncoding::Utf16 => self.inner.to_utf8(
                line_index::WideEncoding::Utf16,
                line_index::WideLineCol {
                    line: position.line,
                    col: position.character,
                },
            )?,
        };
        self.inner.offset(utf8).map(usize::from)
    }
}

/// How a client counts characters within a line. UTF-16 is the protocol default; UTF-8 is
/// negotiated when the client offers it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PositionEncoding {
    Utf8,
    Utf16,
}

/// Zero-based line and character, in whatever encoding was negotiated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProtocolPosition {
    pub line: u32,
    pub character: u32,
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;
