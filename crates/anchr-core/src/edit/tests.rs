use super::*;

#[test]
fn edits_apply_in_order_with_length_changes() {
    let source = "@anchor[ab] @ref[#ab] tail";
    let edits = vec![
        TextEdit {
            span: ByteSpan::new(8, 10),
            expected: "ab".into(),
            replacement: "longer-name".into(),
        },
        TextEdit {
            span: ByteSpan::new(18, 20),
            expected: "ab".into(),
            replacement: "longer-name".into(),
        },
    ];
    assert_eq!(
        apply_edits(source, &edits).unwrap(),
        "@anchor[longer-name] @ref[#longer-name] tail"
    );
}

#[test]
fn a_mismatch_or_overlap_refuses_the_whole_file() {
    let source = "@anchor[ab]";
    let stale = vec![TextEdit {
        span: ByteSpan::new(8, 10),
        expected: "zz".into(),
        replacement: "x".into(),
    }];
    assert_eq!(apply_edits(source, &stale), None);
    let overlapping = vec![
        TextEdit {
            span: ByteSpan::new(0, 5),
            expected: "@anch".into(),
            replacement: "".into(),
        },
        TextEdit {
            span: ByteSpan::new(3, 6),
            expected: "cho".into(),
            replacement: "".into(),
        },
    ];
    assert_eq!(apply_edits(source, &overlapping), None);
}

fn edit(start: usize, end: usize, expected: &str, replacement: &str) -> TextEdit {
    TextEdit {
        span: ByteSpan::new(start, end),
        expected: expected.to_owned(),
        replacement: replacement.to_owned(),
    }
}

fn file(path: &str) -> FilePath {
    FilePath::new(camino::Utf8PathBuf::from(path)).unwrap()
}

struct Fixture {
    _dir: tempfile::TempDir,
    root_dir: camino::Utf8PathBuf,
}

impl Fixture {
    fn new(files: &[(&str, &str)]) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root_dir = camino::Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
        for (path, contents) in files {
            let full = root_dir.join(path);
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(full, contents).unwrap();
        }
        Self {
            _dir: dir,
            root_dir,
        }
    }

    fn read(&self, path: &str) -> String {
        std::fs::read_to_string(self.root_dir.join(path)).unwrap()
    }
}

#[test]
fn every_named_file_is_rewritten_and_reported_in_order() {
    let fixture = Fixture::new(&[("a.md", "see ab here"), ("docs/b.md", "and ab too")]);
    let edits = BTreeMap::from([
        (file("a.md"), vec![edit(4, 6, "ab", "cd")]),
        (file("docs/b.md"), vec![edit(4, 6, "ab", "cd")]),
    ]);

    let written = apply_to_files(&fixture.root_dir, &edits).unwrap();
    assert_eq!(written, vec![file("a.md"), file("docs/b.md")]);
    assert_eq!(fixture.read("a.md"), "see cd here");
    assert_eq!(fixture.read("docs/b.md"), "and cd too");
}

#[test]
fn no_edits_writes_nothing_and_reports_nothing() {
    let fixture = Fixture::new(&[("a.md", "unchanged")]);
    assert!(
        apply_to_files(&fixture.root_dir, &BTreeMap::new())
            .unwrap()
            .is_empty()
    );
    assert_eq!(fixture.read("a.md"), "unchanged");
}

/// The point of recording `expected`: a file edited between the scan and the write is refused
/// rather than corrupted.
#[test]
fn a_file_that_changed_since_the_scan_is_refused() {
    let fixture = Fixture::new(&[("a.md", "see xy here")]);
    let edits = BTreeMap::from([(file("a.md"), vec![edit(4, 6, "ab", "cd")])]);

    let error = apply_to_files(&fixture.root_dir, &edits).unwrap_err();
    assert!(matches!(error, ApplyError::FileChanged { .. }), "{error:?}");
    assert!(error.to_string().contains("a.md"), "{error}");
    assert_eq!(
        fixture.read("a.md"),
        "see xy here",
        "the file must be untouched"
    );
}

#[test]
fn a_file_that_is_not_there_reports_the_read_failure_by_path() {
    let fixture = Fixture::new(&[]);
    let edits = BTreeMap::from([(file("gone.md"), vec![edit(0, 2, "ab", "cd")])]);

    let error = apply_to_files(&fixture.root_dir, &edits).unwrap_err();
    assert!(matches!(error, ApplyError::Read { .. }), "{error:?}");
    assert!(error.to_string().contains("gone.md"), "{error}");
}
