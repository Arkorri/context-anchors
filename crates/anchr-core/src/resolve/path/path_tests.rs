use camino::Utf8PathBuf;

use crate::config::Config;
use crate::root::FilePath;
use crate::tree::Entry;

use super::*;

fn root_at(dir: &Utf8Path) -> Root {
    Root {
        name: RootName::parse("r").unwrap(),
        dir: dir.to_path_buf(),
        config: Config::default(),
    }
}

fn tree_of(paths: &[&str]) -> FileTree {
    let mut tree = FileTree::default();
    for path in paths {
        tree.insert(
            &FilePath::new(Utf8PathBuf::from(path)).unwrap(),
            Entry::File,
        );
    }
    tree
}

fn rel(path: &str) -> RelPath {
    RelPath::parse(path).unwrap()
}

fn located(tree: &FileTree, path: &str) -> Located {
    PathResolver::default().locate(&root_at(Utf8Path::new("/r")), tree, &rel(path))
}

#[test]
fn a_file_in_the_tree_is_found_and_carries_its_absolute_path() {
    let tree = tree_of(&["docs/a.md"]);
    match located(&tree, "docs/a.md") {
        Located::Found { kind, absolute } => {
            assert_eq!(kind, EntryKind::File);
            assert_eq!(absolute, Utf8PathBuf::from("/r").join("docs").join("a.md"));
        }
        Located::Missing { .. } => panic!("expected a file"),
    }
}

#[test]
fn a_directory_implied_by_its_children_is_found() {
    // A `RelPath` never carries the trailing slash; the directory expectation is on the target.
    let tree = tree_of(&["docs/a.md"]);
    match located(&tree, "docs") {
        Located::Found { kind, .. } => assert_eq!(kind, EntryKind::Directory),
        Located::Missing { .. } => panic!("expected a directory"),
    }
}

#[test]
fn a_missing_entry_names_its_deepest_existing_parent() {
    let tree = tree_of(&["docs/a.md"]);
    match located(&tree, "docs/gone.md") {
        Located::Missing { parent, name } => {
            assert_eq!(parent, "docs");
            assert_eq!(name, "gone.md");
        }
        Located::Found { .. } => panic!("expected a miss"),
    }
    match located(&tree, "gone.md") {
        Located::Missing { parent, name } => {
            assert_eq!(parent, "", "a miss at the root has an empty parent");
            assert_eq!(name, "gone.md");
        }
        Located::Found { .. } => panic!("expected a miss"),
    }
}

/// The module's central claim. A file present on disk but absent from the scan is missing, so a
/// gitignored or excluded target cannot resolve, and a local run agrees with a CI checkout.
#[test]
fn existence_follows_the_scan_tree_and_not_the_disk() {
    let dir = tempfile::tempdir().unwrap();
    let dir = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
    std::fs::write(dir.join("on-disk-only.md"), "hi").unwrap();

    let tree = tree_of(&["in-tree-only.md"]);
    let resolver = PathResolver::default();
    let root = root_at(&dir);

    assert!(
        matches!(
            resolver.locate(&root, &tree, &rel("on-disk-only.md")),
            Located::Missing { .. }
        ),
        "a file the scan never saw must not resolve"
    );
    assert!(
        matches!(
            resolver.locate(&root, &tree, &rel("in-tree-only.md")),
            Located::Found { .. }
        ),
        "a file in the tree resolves without being read back from disk"
    );
}

#[test]
fn a_near_miss_sibling_is_suggested_with_its_parent_spliced_back_in() {
    let resolver = PathResolver::default();
    let root = root_at(Utf8Path::new("/r"));

    let tree = tree_of(&["README.md"]);
    assert_eq!(
        resolver.suggest(&root, &tree, &rel("REAMDE.md")),
        Some("README.md".to_owned())
    );

    let tree = tree_of(&["docs/guide.md"]);
    assert_eq!(
        resolver.suggest(&root, &tree, &rel("docs/guied.md")),
        Some("docs/guide.md".to_owned())
    );
}

#[test]
fn nothing_is_suggested_for_a_path_that_resolves_or_has_no_close_sibling() {
    let resolver = PathResolver::default();
    let root = root_at(Utf8Path::new("/r"));
    let tree = tree_of(&["README.md"]);

    assert_eq!(resolver.suggest(&root, &tree, &rel("README.md")), None);
    assert_eq!(
        resolver.suggest(&root, &tree, &rel("completely-different.md")),
        None
    );
}

#[test]
fn a_path_outside_the_root_never_stays_within_it() {
    let dir = tempfile::tempdir().unwrap();
    let dir = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
    let inside = dir.join("inside.md");
    std::fs::write(&inside, "hi").unwrap();

    let mut resolver = PathResolver::default();
    let root = root_at(&dir);
    assert!(resolver.stays_within(&root, &inside));

    let outside = dir.join("..").join("elsewhere.md");
    assert!(
        !resolver.stays_within(&root, &outside),
        "an unreadable or escaping path is refused"
    );
}

#[test]
fn a_root_that_cannot_be_canonicalized_refuses_everything() {
    let mut resolver = PathResolver::default();
    let root = root_at(Utf8Path::new("/no/such/directory/anywhere"));
    assert!(!resolver.stays_within(&root, Utf8Path::new("/no/such/directory/anywhere/a.md")));
    // The failure is cached as `None`; a second call must still refuse rather than retry into a
    // different answer.
    assert!(!resolver.stays_within(&root, Utf8Path::new("/no/such/directory/anywhere/a.md")));
}
