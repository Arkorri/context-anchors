use super::*;

fn parse(text: &str) -> Result<Config, ConfigError> {
    Config::from_toml(text, Utf8Path::new("/repo/anchr.toml"))
}

#[test]
fn an_empty_file_is_the_defaults() {
    let config = parse("").unwrap();
    assert!(config.root_name.is_none());
    assert!(config.external_roots.is_empty());
    assert!(config.scan.include.is_none());
    assert_eq!(config.scan.max_file_bytes, DEFAULT_MAX_FILE_BYTES);
    assert_eq!(config.scan.parse_budget, DEFAULT_PARSE_BUDGET);
    assert_eq!(config.containers, ContainerRules::default());
    assert_eq!(config.check.unverified, UnverifiedPolicy::Report);
    assert!(config.ignore.paths.is_empty());
    assert!(config.ignore.tokens.is_empty());
}

#[test]
fn every_section_parses_with_kebab_case_keys() {
    let config = parse(
        r#"
        [root]
        name = "my-root"

        [roots]
        claude = "~/.claude"
        sibling = "../plugin"
        abs = "/opt/shared"

        [scan]
        include = ["docs/**/*.md"]
        max-file-bytes = 4096
        parse-budget-ms = 250

        [containers]
        markdown = ["MD", "mdx"]
        plaintext = ["txt", "text"]

        [check]
        unverified = "error"

        [ignore]
        paths = ["vendor/**", "/build/", "!vendor/keep.md"]
        tokens = ["CLAUDE.md", "src/legacy/**"]
        "#,
    )
    .unwrap();

    assert_eq!(config.root_name.unwrap().as_str(), "my-root");
    let roots = &config.external_roots;
    assert!(roots[&RootName::parse("claude").unwrap()].is_absolute());
    assert!(roots[&RootName::parse("claude").unwrap()].ends_with(".claude"));
    assert_eq!(
        roots[&RootName::parse("sibling").unwrap()],
        Utf8PathBuf::from("/repo/../plugin")
    );
    assert_eq!(
        roots[&RootName::parse("abs").unwrap()],
        Utf8PathBuf::from("/opt/shared")
    );
    assert!(config.scan.include.unwrap().is_match("docs/a/b.md"));
    assert_eq!(config.scan.max_file_bytes, 4096);
    assert_eq!(config.scan.parse_budget, Duration::from_millis(250));
    assert_eq!(config.containers.markdown_extensions, vec!["md", "mdx"]);
    assert_eq!(config.containers.plaintext_extensions, vec!["txt", "text"]);
    assert_eq!(config.check.unverified, UnverifiedPolicy::Error);
    let paths = &config.ignore.paths;
    assert!(paths.matched("vendor/lib.md", false).is_ignore());
    assert!(paths.matched("vendor/keep.md", false).is_whitelist());
    assert!(paths.matched("build", true).is_ignore());
    assert!(paths.matched("docs/build", true).is_none());
    assert_eq!(
        config
            .ignore
            .tokens
            .iter()
            .map(NoRefEntry::as_str)
            .collect::<Vec<_>>(),
        vec!["CLAUDE.md", "src/legacy/**"]
    );
}

#[test]
fn unknown_keys_are_a_parse_error_with_a_span() {
    let error = parse("[scan]\ninclud = []\n").unwrap_err();
    let ConfigError::Parse { source, .. } = error else {
        panic!("expected parse error, got {error}");
    };
    assert!(source.span().is_some());
    assert!(source.message().contains("includ"));
}

#[test]
fn semantic_errors_name_the_field_and_carry_a_span() {
    let cases = [
        ("[root]\nname = \"has space\"\n", "root.name"),
        ("[roots]\n\"bad.name\" = \"x\"\n", "roots.bad.name"),
        ("[roots]\nok = \"\"\n", "roots.ok"),
        ("[scan]\nmax-file-bytes = 0\n", "scan.max-file-bytes"),
        (
            "[scan]\nmax-file-bytes = 4294967296\n",
            "scan.max-file-bytes",
        ),
        ("[scan]\nparse-budget-ms = 0\n", "scan.parse-budget-ms"),
        (
            "[containers]\nmarkdown = [\".md\"]\n",
            "containers.markdown",
        ),
        ("[ignore]\npaths = [\"\"]\n", "ignore.paths"),
        ("[ignore]\npaths = [\"# comment\"]\n", "ignore.paths"),
        ("[ignore]\ntokens = [\"\"]\n", "ignore.tokens"),
        ("[ignore]\ntokens = [\"a b\"]\n", "ignore.tokens"),
        ("[ignore]\ntokens = [\"docs/[\"]\n", "ignore.tokens"),
        ("[ignore]\ntokens = [\"x\", \"x\"]\n", "ignore.tokens"),
    ];
    for (text, expected_field) in cases {
        match parse(text) {
            Err(ConfigError::Invalid { field, span, .. }) => {
                assert_eq!(field, expected_field, "for {text:?}");
                assert!(span.is_some(), "no span for {text:?}");
            }
            other => panic!("expected Invalid for {text:?}, got {other:?}"),
        }
    }
}

#[test]
fn an_invalid_glob_is_rejected() {
    let error = parse("[scan]\ninclude = [\"docs/[\"]\n").unwrap_err();
    assert!(matches!(error, ConfigError::Invalid { field, .. } if field == "scan.include"));
}

#[test]
fn globs_do_not_let_star_cross_directories() {
    let config = parse("[scan]\ninclude = [\"docs/*.md\"]\n").unwrap();
    let include = config.scan.include.unwrap();
    assert!(include.is_match("docs/a.md"));
    assert!(!include.is_match("docs/sub/a.md"));
}

#[test]
fn home_expansion_only_applies_to_a_leading_tilde_segment() {
    assert_eq!(expand_home("~x/y").unwrap(), Utf8PathBuf::from("~x/y"));
    assert_eq!(expand_home("a/~/b").unwrap(), Utf8PathBuf::from("a/~/b"));
    let home = home_dir().unwrap();
    assert_eq!(expand_home("~").unwrap(), home);
    assert_eq!(expand_home("~/.claude").unwrap(), home.join(".claude"));
}

#[test]
fn path_pattern_names_the_gitignore_line_that_removed_a_path() {
    let config = parse(
        "[ignore]\npaths = [\"target/**\", \"**/snapshots/**\", \"!target/keep.md\", \"drafts/\"]\n",
    )
    .unwrap();
    let ignore = &config.ignore;
    let pattern = |path: &str, is_dir: bool| ignore.path_pattern(Utf8Path::new(path), is_dir);
    assert_eq!(pattern("target/debug/x.md", false), Some("target/**"));
    assert_eq!(pattern("target", true), Some("target/**"));
    assert_eq!(pattern("target", false), None);
    assert_eq!(pattern("target/keep.md", false), None);
    assert_eq!(
        pattern("a/snapshots/b.snap", false),
        Some("**/snapshots/**")
    );
    assert_eq!(pattern("drafts", true), Some("drafts/"));
    assert_eq!(pattern("docs/drafts", true), Some("drafts/"));
    assert_eq!(pattern("drafts", false), None);
    assert_eq!(pattern("src/a.md", false), None);
    assert_eq!(
        Config::default()
            .ignore
            .path_pattern(Utf8Path::new("target/x"), false),
        None
    );
}

#[test]
fn discovery_prefers_the_nearest_config_then_git_then_start() {
    let dir = tempfile::tempdir().unwrap();
    let base = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
    let repo = base.join("repo");
    let nested = repo.join("a/b");
    std::fs::create_dir_all(&nested).unwrap();

    let found = discover(&nested).unwrap();
    assert_eq!(found.root_dir, nested);
    assert!(found.config_path.is_none());

    std::fs::create_dir(repo.join(".git")).unwrap();
    let found = discover(&nested).unwrap();
    assert_eq!(found.root_dir, repo);
    assert!(found.config_path.is_none());

    std::fs::write(
        repo.join("a").join(CONFIG_FILE_NAME),
        "[root]\nname = \"inner\"\n",
    )
    .unwrap();
    let found = discover(&nested).unwrap();
    assert_eq!(found.root_dir, repo.join("a"));
    assert_eq!(found.config.root_name.unwrap().as_str(), "inner");
    assert!(found.config_path.unwrap().ends_with(CONFIG_FILE_NAME));
}
