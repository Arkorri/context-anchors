use camino::Utf8PathBuf;

use super::*;
use crate::config::Config;
use crate::root::RootName;
use crate::tree::Lookup;

struct Fixture {
    _dir: tempfile::TempDir,
    root: Root,
}

impl Fixture {
    fn new(files: &[(&str, &str)]) -> Self {
        Self::with_config(files, Config::default())
    }

    fn with_config(files: &[(&str, &str)], config: Config) -> Self {
        let fixture = Self::without_git(files, config);
        std::fs::create_dir_all(fixture.root.dir.join(".git")).unwrap();
        fixture
    }

    /// A root that is not a git repository, so `.gitignore` files in it carry no weight.
    fn without_git(files: &[(&str, &str)], config: Config) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let base = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
        for (path, contents) in files {
            let full = base.join(path);
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(full, contents).unwrap();
        }
        Self {
            _dir: dir,
            root: Root {
                name: RootName::parse("fixture").unwrap(),
                dir: base,
                config,
            },
        }
    }

    fn scan(&self, mode: ScanMode) -> ScanOutput {
        let registry = LanguageRegistry::new().unwrap();
        scan_root(&self.root, &registry, mode)
    }
}

/// A config whose `[ignore] paths` holds `lines`, rooted where the fixture will live. The
/// matcher strips the root prefix itself, so any root works for relative matching.
fn ignoring(lines: &[&str]) -> Config {
    let mut builder = ignore::gitignore::GitignoreBuilder::new("/fixture");
    for line in lines {
        builder.add_line(None, line).unwrap();
    }
    let mut config = Config::default();
    config.ignore.paths = builder.build().unwrap();
    config
}

fn paths(files: &[ScannedFile]) -> Vec<&str> {
    files.iter().map(|file| file.path.as_str()).collect()
}

#[test]
fn scans_opted_in_containers_and_ignores_everything_else() {
    let fixture = Fixture::new(&[
        ("README.md", "@anchor[readme]"),
        ("notes.txt", "@ref[README.md]"),
        ("src/lib.rs", "// @ref[#readme]"),
        ("Makefile", "@ref[not-scanned]"),
        ("image.png", "@ref[not-scanned]"),
    ]);
    let output = fixture.scan(ScanMode::Full);
    assert_eq!(
        paths(&output.files),
        vec!["README.md", "notes.txt", "src/lib.rs"]
    );
    assert!(output.skipped.is_empty());
    assert!(output.problems.is_empty());
    let total: usize = output.files.iter().map(|f| f.scan.markers.len()).sum();
    assert_eq!(total, 3);
}

#[test]
fn gitignore_is_honoured_and_include_cannot_override_it() {
    let mut config = Config::default();
    config.scan.include = Some(
        globset::GlobSetBuilder::new()
            .add(globset::Glob::new("**/*.md").unwrap())
            .build()
            .unwrap(),
    );
    let fixture = Fixture::with_config(
        &[
            (".gitignore", "generated.md\n"),
            ("kept.md", ""),
            ("generated.md", ""),
            ("notes.txt", "not included"),
        ],
        config,
    );
    assert_eq!(paths(&fixture.scan(ScanMode::Full).files), vec!["kept.md"]);
}

#[test]
fn visited_extensions_are_harvested_before_the_include_filter() {
    let mut config = Config::default();
    config.scan.include = Some(
        globset::GlobSetBuilder::new()
            .add(globset::Glob::new("**/*.md").unwrap())
            .build()
            .unwrap(),
    );
    let fixture = Fixture::with_config(
        &[
            (".gitignore", "gen.zzz\n"),
            ("a.md", ""),
            ("Cargo.lock", ""),
            ("img.PNG", ""),
            ("Makefile", ""),
            ("gen.zzz", ""),
        ],
        config,
    );
    let output = fixture.scan(ScanMode::Full);
    assert_eq!(paths(&output.files), vec!["a.md"]);
    assert_eq!(
        output.extensions,
        ["lock", "md", "png"]
            .map(str::to_owned)
            .into_iter()
            .collect()
    );
    assert!(fixture.scan(ScanMode::AnchorsOnly).extensions.is_empty());
}

#[test]
fn hidden_files_are_scanned_and_git_internals_are_not() {
    let fixture = Fixture::new(&[
        (".claude/skills/x/SKILL.md", "@anchor[skill]"),
        (".github/workflows/ci.yml", ""),
        (".git/COMMIT_EDITMSG", "@anchor[leak]"),
        (".git/hooks/pre-commit.sample", ""),
        ("kept.md", ""),
    ]);
    let output = fixture.scan(ScanMode::Full);
    assert_eq!(
        paths(&output.files),
        vec![".claude/skills/x/SKILL.md", "kept.md"]
    );
    assert_eq!(
        output.extensions,
        ["md", "yml"].map(str::to_owned).into_iter().collect()
    );
    let total: usize = output.files.iter().map(|f| f.scan.markers.len()).sum();
    assert_eq!(total, 1);
}

#[test]
fn gitignored_dotfiles_are_still_skipped() {
    let fixture = Fixture::new(&[
        (".gitignore", ".cache/\n.secret.md\n"),
        (".cache/notes.md", ""),
        (".secret.md", ""),
        ("kept.md", ""),
    ]);
    assert_eq!(paths(&fixture.scan(ScanMode::Full).files), vec!["kept.md"]);
}

#[test]
fn ignore_paths_prune_files_and_cannot_whitelist_gitignored_ones() {
    let fixture = Fixture::with_config(
        &[
            (".gitignore", "build/\n"),
            ("kept.md", ""),
            ("vendor/lib.md", ""),
            ("vendor/keep.md", ""),
            ("drafts/wip.md", ""),
            ("docs/drafts/wip.md", ""),
            ("build/x.md", ""),
        ],
        ignoring(&["vendor/**", "!vendor/keep.md", "drafts/", "!build/x.md"]),
    );
    let output = fixture.scan(ScanMode::Full);
    assert_eq!(paths(&output.files), vec!["kept.md", "vendor/keep.md"]);
    assert!(matches!(
        output.tree.lookup("drafts".into()),
        Lookup::Missing { .. }
    ));
}

#[test]
fn a_root_without_git_gets_no_gitignore_processing() {
    let fixture = Fixture::without_git(
        &[
            (".gitignore", "drafts/\n"),
            ("drafts/wip.md", ""),
            ("kept.md", ""),
        ],
        ignoring(&["vendor/**"]),
    );
    assert_eq!(
        paths(&fixture.scan(ScanMode::Full).files),
        vec!["drafts/wip.md", "kept.md"]
    );
}

#[test]
fn a_parent_gitignore_stops_at_the_nearest_git_directory() {
    let fixture = Fixture::without_git(&[(".gitignore", "inner/**\n")], Config::default());
    let inner = fixture.root.dir.join("inner");
    std::fs::create_dir_all(inner.join(".git")).unwrap();
    std::fs::write(inner.join("kept.md"), "").unwrap();
    std::fs::write(inner.join(".gitignore"), "drafts/\n").unwrap();
    std::fs::create_dir_all(inner.join("drafts")).unwrap();
    std::fs::write(inner.join("drafts/wip.md"), "").unwrap();
    let root = Root {
        name: RootName::parse("inner").unwrap(),
        dir: inner,
        config: Config::default(),
    };
    let registry = LanguageRegistry::new().unwrap();
    let output = scan_root(&root, &registry, ScanMode::Full);
    assert_eq!(paths(&output.files), vec!["kept.md"]);
}

#[test]
fn oversized_and_non_utf8_files_are_skipped_with_a_reason() {
    let mut config = Config::default();
    config.scan.max_file_bytes = 16;
    let fixture = Fixture::with_config(
        &[
            ("small.md", "@anchor[a]"),
            ("big.md", "x".repeat(17).as_str()),
        ],
        config,
    );
    std::fs::write(fixture.root.dir.join("binary.md"), [0xff, 0xfe, b'@']).unwrap();

    let output = fixture.scan(ScanMode::Full);
    assert_eq!(paths(&output.files), vec!["small.md"]);
    let reasons: Vec<(&str, &SkipReason)> = output
        .skipped
        .iter()
        .map(|s| (s.path.as_str(), &s.reason))
        .collect();
    assert!(matches!(
        reasons[0],
        (
            "big.md",
            SkipReason::TooLarge {
                bytes: 17,
                limit: 16
            }
        )
    ));
    assert!(matches!(reasons[1], ("binary.md", SkipReason::NotUtf8)));
}

#[test]
fn files_with_unreferenceable_names_are_still_scanned() {
    let fixture = Fixture::new(&[("My Notes #1.md", "@anchor[notes]")]);
    let output = fixture.scan(ScanMode::Full);
    assert_eq!(paths(&output.files), vec!["My Notes #1.md"]);
    assert_eq!(output.files[0].scan.markers.len(), 1);
}

#[test]
fn anchors_only_mode_drops_references_and_malformed_markers() {
    let fixture = Fixture::new(&[("a.md", "@anchor[a] @ref[#b] @ref[")]);
    let full = fixture.scan(ScanMode::Full);
    assert_eq!(full.files[0].scan.markers.len(), 2);
    assert_eq!(full.files[0].scan.malformed.len(), 1);

    let anchors_only = fixture.scan(ScanMode::AnchorsOnly);
    assert_eq!(anchors_only.files[0].scan.markers.len(), 1);
    assert!(anchors_only.files[0].scan.malformed.is_empty());
}

#[cfg(unix)]
#[test]
fn symlinked_directories_are_not_followed_but_are_recorded() {
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("secret.md"), "@anchor[secret]").unwrap();
    let fixture = Fixture::new(&[("kept.md", "")]);
    std::os::unix::fs::symlink(outside.path(), fixture.root.dir.join("linked")).unwrap();
    let output = fixture.scan(ScanMode::Full);
    assert_eq!(paths(&output.files), vec!["kept.md"]);
    assert_eq!(output.tree.lookup("linked".into()), Lookup::LeavesRoot);
    assert_eq!(output.tree.len(), 2);
}

#[cfg(unix)]
#[test]
fn the_tree_records_walked_files_and_symlinks_only() {
    let config = ignoring(&["vendor/**"]);
    let fixture = Fixture::with_config(
        &[
            (".gitignore", "build/\n"),
            ("a.md", "@anchor[a]"),
            ("Cargo.lock", ""),
            ("sub/deep/x.txt", ""),
            ("vendor/lib.md", ""),
            ("build/out.md", ""),
        ],
        config,
    );
    std::fs::create_dir_all(fixture.root.dir.join("empty")).unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink("a.md", fixture.root.dir.join("link.md")).unwrap();
    std::os::unix::fs::symlink(outside.path(), fixture.root.dir.join("outside")).unwrap();

    for mode in [ScanMode::Full, ScanMode::AnchorsOnly] {
        let tree = fixture.scan(mode).tree;
        assert_eq!(tree.lookup("Cargo.lock".into()), Lookup::File);
        assert_eq!(tree.lookup(".gitignore".into()), Lookup::File);
        assert_eq!(tree.lookup("sub".into()), Lookup::Directory);
        assert_eq!(tree.lookup("sub/deep".into()), Lookup::Directory);
        assert_eq!(tree.lookup("link.md".into()), Lookup::File);
        assert_eq!(tree.lookup("outside".into()), Lookup::LeavesRoot);
        for absent in ["vendor/lib.md", "vendor", "build/out.md", "build", "empty"] {
            assert!(
                matches!(tree.lookup(absent.into()), Lookup::Missing { .. }),
                "{absent} in {mode:?}"
            );
        }
    }
}
