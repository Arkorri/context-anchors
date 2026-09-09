use super::*;

fn tree(files: &[&str], links: &[(&str, &str)]) -> FileTree {
    let mut tree = FileTree::default();
    for file in files {
        tree.insert(
            &FilePath::new(Utf8PathBuf::from(file)).unwrap(),
            Entry::File,
        );
    }
    for (link, target) in links {
        tree.insert(
            &FilePath::new(Utf8PathBuf::from(link)).unwrap(),
            Entry::Symlink {
                target: Utf8PathBuf::from(target),
            },
        );
    }
    tree
}

fn missing(parent: &str, name: &str) -> Lookup {
    Lookup::Missing {
        parent: parent.to_owned(),
        name: name.to_owned(),
    }
}

#[test]
fn files_directories_and_prefixes_are_told_apart_by_exact_bytes() {
    let t = tree(&["docs/guide.md", "docs2/x.md", "Cargo.lock"], &[]);
    assert_eq!(t.lookup("docs/guide.md".into()), Lookup::File);
    assert_eq!(t.lookup("Cargo.lock".into()), Lookup::File);
    assert_eq!(t.lookup("docs".into()), Lookup::Directory);
    assert_eq!(t.lookup("".into()), Lookup::Directory);
    assert_eq!(t.lookup("doc".into()), missing("", "doc"));
    assert_eq!(t.lookup("docs2/y.md".into()), missing("docs2", "y.md"));
    assert_eq!(
        t.lookup("docs/guide.md/x".into()),
        missing("docs/guide.md", "x")
    );
    assert_eq!(t.lookup("Docs/guide.md".into()), missing("", "Docs"));
    assert_eq!(t.lookup("docs/sub/deep.md".into()), missing("docs", "sub"));
    assert!(FileTree::default().lookup("".into()) != Lookup::Directory);
}

#[test]
fn children_and_basenames_come_from_the_tree() {
    let t = tree(
        &["docs/guide.md", "docs/sub/x.md", "src/lib.rs", "guide.md"],
        &[("docs/link", "sub")],
    );
    assert_eq!(
        t.children("docs"),
        ["guide.md", "link", "sub"].into_iter().collect()
    );
    assert_eq!(
        t.children(""),
        ["docs", "guide.md", "src"].into_iter().collect()
    );
    assert_eq!(t.children("nowhere"), BTreeSet::new());
    assert_eq!(t.by_basename("guide.md"), vec!["docs/guide.md", "guide.md"]);
    assert_eq!(t.by_basename("x.md"), vec!["docs/sub/x.md"]);
    assert!(t.by_basename("nope.md").is_empty());
}

#[test]
fn symlinks_redirect_lexically_and_never_leave_the_root() {
    let t = tree(
        &["docs/guide.md", "c.md"],
        &[
            ("a", "b"),
            ("b", "c.md"),
            ("loop1", "loop2"),
            ("loop2", "loop1"),
            ("up", "../x"),
            ("abs", "/etc"),
            ("dangling", "target/x.md"),
            ("link", "docs"),
            ("CLAUDE.md", "AGENTS.md"),
        ],
    );
    assert_eq!(t.lookup("a".into()), Lookup::File);
    assert_eq!(t.lookup("link".into()), Lookup::Directory);
    assert_eq!(t.lookup("link/guide.md".into()), Lookup::File);
    assert_eq!(t.lookup("link/nope.md".into()), missing("docs", "nope.md"));
    assert_eq!(t.lookup("loop1".into()), missing("", "loop1"));
    assert_eq!(t.lookup("up".into()), Lookup::LeavesRoot);
    assert_eq!(t.lookup("up/deeper.md".into()), Lookup::LeavesRoot);
    assert_eq!(t.lookup("abs".into()), Lookup::LeavesRoot);
    assert_eq!(t.lookup("dangling".into()), missing("", "target"));
    assert_eq!(t.lookup("CLAUDE.md".into()), missing("", "AGENTS.md"));
}

#[test]
fn normalize_collapses_dots_and_rejects_escapes() {
    let n = |s: &str| normalize(Utf8Path::new(s)).map(|p| p.to_string());
    assert_eq!(n("docs/./guide.md"), Some("docs/guide.md".to_owned()));
    assert_eq!(n("docs/sub/../guide.md"), Some("docs/guide.md".to_owned()));
    assert_eq!(n("docs/.."), Some(String::new()));
    assert_eq!(n("a/b/../../c"), Some("c".to_owned()));
    assert_eq!(n(".."), None);
    assert_eq!(n("docs/../../x"), None);
    assert_eq!(n("/etc/passwd"), None);
}
