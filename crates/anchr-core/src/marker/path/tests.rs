use camino::Utf8Component;
use proptest::prelude::*;

use super::*;

#[test]
fn accepts_ordinary_relative_paths() {
    let path = RelPath::parse("src/auth/provider.ts").unwrap();
    assert_eq!(path.extension(), Some("ts"));
    assert_eq!(path.parent().unwrap().as_str(), "src/auth");
    assert_eq!(path.file_name(), "provider.ts");
    assert_eq!(RelPath::parse("README").unwrap().parent(), None);
    assert_eq!(
        RelPath::parse("données/façade.md").unwrap().as_str(),
        "données/façade.md"
    );
}

#[test]
fn rejects_each_malformed_shape() {
    assert_eq!(RelPath::parse(""), Err(PathError::Empty));
    assert_eq!(RelPath::parse("/etc/passwd"), Err(PathError::Absolute));
    assert_eq!(RelPath::parse("a\\b"), Err(PathError::Backslash));
    assert_eq!(RelPath::parse("a//b"), Err(PathError::EmptySegment));
    assert_eq!(RelPath::parse("a/"), Err(PathError::EmptySegment));
    assert_eq!(RelPath::parse("./a"), Err(PathError::CurrentDirectory));
    assert_eq!(RelPath::parse("a/../b"), Err(PathError::ParentDirectory));
    assert_eq!(
        RelPath::parse("C:/x"),
        Err(PathError::ReservedChar { ch: ':' })
    );
    assert_eq!(
        RelPath::parse("a#b"),
        Err(PathError::ReservedChar { ch: '#' })
    );
    assert_eq!(
        RelPath::parse("a b"),
        Err(PathError::InvalidChar { ch: ' ' })
    );
    assert_eq!(
        RelPath::parse("a\tb"),
        Err(PathError::InvalidChar { ch: '\t' })
    );
}

#[test]
fn anchored_resolves_dot_forms_against_the_file_directory() {
    let docs = Utf8Path::new("docs/design");
    let anchored = |dir: &str, written: &str| {
        RelPath::anchored(Utf8Path::new(dir), written).map(|p| p.as_str().to_owned())
    };
    assert_eq!(
        anchored("docs/design", "./x.md"),
        Ok("docs/design/x.md".to_owned())
    );
    assert_eq!(
        anchored("docs/design", "../x.md"),
        Ok("docs/x.md".to_owned())
    );
    assert_eq!(anchored("docs/design", "../../x.md"), Ok("x.md".to_owned()));
    assert_eq!(anchored("docs/design", ".."), Ok("docs".to_owned()));
    assert_eq!(
        anchored("docs", "./sub/x.md"),
        Ok("docs/sub/x.md".to_owned())
    );
    assert_eq!(anchored("", "./x.md"), Ok("x.md".to_owned()));
    assert_eq!(anchored("docs", ".hidden"), Ok(".hidden".to_owned()));
    assert_eq!(anchored("docs", "..rc"), Ok("..rc".to_owned()));
    assert_eq!(
        RelPath::anchored(docs, "x.md"),
        RelPath::parse("x.md"),
        "a bare spelling stays root-relative wherever it is written"
    );

    assert_eq!(
        anchored("docs/design", "../../../x.md"),
        Err(PathError::EscapesRoot)
    );
    assert_eq!(anchored("", "../x.md"), Err(PathError::EscapesRoot));
    assert_eq!(anchored("", "."), Err(PathError::NamesRoot));
    assert_eq!(anchored("docs", ".."), Err(PathError::NamesRoot));
    assert_eq!(
        anchored("docs", "./a/./b"),
        Err(PathError::CurrentDirectory)
    );
    assert_eq!(anchored("docs", "./../x"), Err(PathError::ParentDirectory));
    assert_eq!(
        anchored("docs", "../a/../b"),
        Err(PathError::ParentDirectory)
    );
    assert_eq!(anchored("docs", "a/../b"), Err(PathError::ParentDirectory));
    assert_eq!(anchored("docs", "./a//b"), Err(PathError::EmptySegment));
    assert_eq!(
        anchored("docs", "./a b"),
        Err(PathError::InvalidChar { ch: ' ' })
    );
}

proptest! {
    #[test]
    fn a_parsed_path_joined_onto_a_root_never_escapes_it(raw in "\\PC{0,40}") {
        if let Ok(path) = RelPath::parse(&raw) {
            let root = Utf8Path::new("/root");
            let joined = root.join(path.as_path());
            prop_assert!(joined.starts_with(root));
            prop_assert!(joined.components().all(|component| !matches!(
                component,
                Utf8Component::ParentDir | Utf8Component::CurDir
            )));
        }
    }

    #[test]
    fn an_anchored_path_never_escapes_the_root_and_matches_parse_for_bare_spellings(
        dir in "([a-z]{1,4}(/[a-z]{1,4}){0,3})?",
        raw in "\\PC{0,40}",
    ) {
        let file_dir = Utf8Path::new(&dir);
        if let Ok(path) = RelPath::anchored(file_dir, &raw) {
            let root = Utf8Path::new("/root");
            let joined = root.join(path.as_path());
            prop_assert!(joined.starts_with(root));
            prop_assert!(joined != root);
            prop_assert!(joined.components().all(|component| !matches!(
                component,
                Utf8Component::ParentDir | Utf8Component::CurDir
            )));
        }
        if !is_file_relative(&raw) {
            prop_assert_eq!(RelPath::anchored(file_dir, &raw), RelPath::parse(&raw));
        }
    }
}
