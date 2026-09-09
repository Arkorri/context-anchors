use super::*;

/// An absolute path the host platform actually agrees is absolute: on Windows that needs a drive
/// prefix, and `absolute` branches on `is_absolute`.
fn abs(path: &str) -> Utf8PathBuf {
    if cfg!(windows) {
        Utf8PathBuf::from(format!(r"C:\{}", path.replace('/', r"\")))
    } else {
        Utf8PathBuf::from(format!("/{path}"))
    }
}

#[test]
fn a_relative_path_is_joined_onto_the_working_directory() {
    let cwd = abs("r");
    assert_eq!(
        absolute(&cwd, Utf8Path::new("a/b.md")),
        cwd.join("a").join("b.md")
    );
}

#[test]
fn an_absolute_path_ignores_the_working_directory() {
    let elsewhere = abs("other/x.md");
    assert_eq!(absolute(&abs("r"), &elsewhere), elsewhere);
}

#[test]
fn current_and_parent_segments_collapse_lexically() {
    let cwd = abs("r");
    assert_eq!(
        absolute(&cwd, Utf8Path::new("./a/./b.md")),
        cwd.join("a").join("b.md")
    );
    assert_eq!(absolute(&cwd, Utf8Path::new("a/../b.md")), cwd.join("b.md"));
    assert_eq!(
        absolute(&abs("r/sub"), Utf8Path::new("../x.md")),
        abs("r").join("x.md")
    );
}

#[test]
fn parent_segments_clamp_at_the_filesystem_root() {
    // `pop` is a no-op once only the root remains, so a path with more `..` than depth stays
    // absolute instead of degrading into a relative one that a containment check could mistake
    // for a path inside the root.
    let escaped = absolute(&abs("r"), Utf8Path::new("../../../etc/passwd"));
    assert!(escaped.is_absolute(), "{escaped}");
    assert_eq!(escaped, abs("etc").join("passwd"));
}

#[test]
fn no_paths_yields_no_files() {
    assert_eq!(
        files_in_root(&abs("r"), &abs("r"), &[]).unwrap(),
        Vec::new()
    );
}

#[test]
fn a_path_inside_the_root_is_kept_relative_to_it() {
    let files = files_in_root(&abs("r"), &abs("r"), &[Utf8PathBuf::from("docs/a.md")]).unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].as_path(), Utf8Path::new("docs/a.md"));
}

#[test]
fn a_path_outside_the_root_names_both_the_path_and_the_root() {
    let error = files_in_root(&abs("r"), &abs("r"), &[abs("elsewhere/a.md")]).unwrap_err();
    let message = error.to_string();
    assert!(message.contains("is outside the root"), "{message}");
    assert!(message.contains(abs("r").as_str()), "{message}");
}

#[test]
fn the_root_directory_itself_is_not_a_file_path_inside_the_root() {
    let error = files_in_root(&abs("r"), &abs("r"), &[abs("r")]).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("is not a file path inside the root"),
        "{error:#}"
    );
}
