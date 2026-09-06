//! Translation between anchr's byte spans and root-relative paths and the protocol's
//! positions and URIs.

use anchr_core::root::FilePath;
use anchr_core::span::{ByteSpan, LineIndex, PositionEncoding, ProtocolPosition};
use camino::{Utf8Path, Utf8PathBuf};
use ls_types::{Position, Range, Uri};

pub fn range(index: &LineIndex, span: ByteSpan, encoding: PositionEncoding) -> Option<Range> {
    let start = index.protocol_position(span.start, encoding).ok()?;
    let end = index.protocol_position(span.end, encoding).ok()?;
    Some(Range {
        start: position(start),
        end: position(end),
    })
}

pub fn position(position: ProtocolPosition) -> Position {
    Position {
        line: position.line,
        character: position.character,
    }
}

pub fn offset(index: &LineIndex, position: Position, encoding: PositionEncoding) -> Option<usize> {
    index.offset_of(
        ProtocolPosition {
            line: position.line,
            character: position.character,
        },
        encoding,
    )
}

pub fn uri_for(root_dir: &Utf8Path, path: &FilePath) -> Option<Uri> {
    Uri::from_file_path(root_dir.join(path.as_path()).as_std_path())
}

/// The root-relative file path a URI names, or `None` when it is not a file inside `root_dir`.
pub fn file_path_in(root_dir: &Utf8Path, uri: &Uri) -> Option<FilePath> {
    let absolute = uri.to_file_path()?;
    let absolute = Utf8PathBuf::from_path_buf(absolute.into_owned()).ok()?;
    let relative = absolute.strip_prefix(root_dir).ok()?;
    FilePath::new(relative.to_path_buf()).ok()
}
