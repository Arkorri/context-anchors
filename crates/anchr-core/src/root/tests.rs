use super::*;

#[test]
fn root_names_accept_the_documented_charset() {
    assert_eq!(
        RootName::parse("claude-Code_2").unwrap().as_str(),
        "claude-Code_2"
    );
}

#[test]
fn root_names_reject_empty_dots_slashes_and_overlong() {
    assert_eq!(RootName::parse(""), Err(RootNameError::Empty));
    assert_eq!(
        RootName::parse("a.b"),
        Err(RootNameError::InvalidChar { ch: '.' })
    );
    assert_eq!(
        RootName::parse("a/b"),
        Err(RootNameError::InvalidChar { ch: '/' })
    );
    let long = "x".repeat(MAX_ROOT_NAME_BYTES + 1);
    assert_eq!(
        RootName::parse(&long),
        Err(RootNameError::TooLong { len: 65 })
    );
}

#[test]
fn file_paths_allow_any_relative_utf8_name() {
    let path = FilePath::new(Utf8PathBuf::from("docs/My Notes #1.md")).unwrap();
    assert_eq!(path.extension(), Some("md"));
    assert!(matches!(
        FilePath::new(Utf8PathBuf::from("/abs")),
        Err(FilePathError::NotRelative { .. })
    ));
    assert!(matches!(
        FilePath::new(Utf8PathBuf::from("a/../b")),
        Err(FilePathError::NotRelative { .. })
    ));
    assert_eq!(
        FilePath::new(Utf8PathBuf::from("")),
        Err(FilePathError::Empty)
    );
}

#[cfg(windows)]
#[test]
fn file_paths_use_forward_slashes_on_windows() {
    let path = FilePath::new(Utf8PathBuf::from("src\\lib.rs")).unwrap();
    assert_eq!(path.as_str(), "src/lib.rs");
}

#[test]
fn a_rel_path_converts_to_a_file_path() {
    let rel = RelPath::parse("src/lib.rs").unwrap();
    assert_eq!(FilePath::from(&rel).as_str(), "src/lib.rs");
}

#[test]
fn root_set_names_the_current_root_after_its_directory() {
    let dir = tempfile::tempdir().unwrap();
    let project = Utf8PathBuf::from_path_buf(dir.path().join("my-project")).unwrap();
    std::fs::create_dir(&project).unwrap();
    let set = RootSet::load(project.clone(), Config::default()).unwrap();
    assert_eq!(set.current_name().as_str(), "my-project");
    assert_eq!(set.current().dir, project);
    assert_eq!(set.names().count(), 1);
}

#[test]
fn an_unnameable_directory_needs_an_explicit_root_name() {
    let dir = tempfile::tempdir().unwrap();
    let project = Utf8PathBuf::from_path_buf(dir.path().join("my.project")).unwrap();
    std::fs::create_dir(&project).unwrap();
    assert!(matches!(
        RootSet::load(project.clone(), Config::default()),
        Err(RootSetError::InvalidCurrentName { .. })
    ));
    let config = Config {
        root_name: Some(RootName::parse("explicit").unwrap()),
        ..Config::default()
    };
    assert_eq!(
        RootSet::load(project, config)
            .unwrap()
            .current_name()
            .as_str(),
        "explicit"
    );
}

#[test]
fn external_roots_are_present_or_absent_and_may_not_collide() {
    let dir = tempfile::tempdir().unwrap();
    let base = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
    let project = base.join("project");
    let plugin = base.join("plugin");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::create_dir_all(&plugin).unwrap();

    let mut config = Config::default();
    config
        .external_roots
        .insert(RootName::parse("plugin").unwrap(), plugin.clone());
    config
        .external_roots
        .insert(RootName::parse("missing").unwrap(), base.join("nope"));
    let set = RootSet::load(project.clone(), config).unwrap();

    assert!(matches!(
        set.status(&RootName::parse("plugin").unwrap()),
        Some(RootStatus::Present(root)) if root.dir == plugin
    ));
    assert!(matches!(
        set.status(&RootName::parse("missing").unwrap()),
        Some(RootStatus::Absent { .. })
    ));
    assert_eq!(set.present().count(), 2);
    assert_eq!(set.external().count(), 2);

    let mut colliding = Config::default();
    colliding
        .external_roots
        .insert(RootName::parse("project").unwrap(), plugin);
    assert!(matches!(
        RootSet::load(project, colliding),
        Err(RootSetError::Collision { .. })
    ));
}
