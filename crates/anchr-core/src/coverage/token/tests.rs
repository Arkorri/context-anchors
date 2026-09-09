use super::*;

fn known(repo: &[&str]) -> BTreeSet<String> {
    repo.iter().map(|e| (*e).to_owned()).collect()
}

fn shape(token: &str, repo: &BTreeSet<String>) -> Option<PathShape> {
    path_shape(token, KnownExtensions::new(repo))
}

fn prose_tokens(text: &str, repo: &BTreeSet<String>) -> Vec<(String, PathShape)> {
    path_tokens(text, KnownExtensions::new(repo))
        .map(|token| match token.shape {
            Shape::Path(shape) => (token.text, shape),
            Shape::Alias(_) => unreachable!("path tokens only"),
        })
        .collect()
}

#[test]
fn path_shape_classifies_files_directories_and_globs() {
    let repo = known(&[]);
    assert_eq!(shape("file.txt", &repo), Some(PathShape::File));
    assert_eq!(
        shape("src/directory/file.txt", &repo),
        Some(PathShape::File)
    );
    assert_eq!(shape("src/x.rs#run", &repo), Some(PathShape::File));
    assert_eq!(shape("src/directory/", &repo), Some(PathShape::Directory));
    assert_eq!(shape("src/", &repo), Some(PathShape::Directory));
    assert_eq!(shape("src/*", &repo), Some(PathShape::Glob));
    assert_eq!(shape("src/**", &repo), Some(PathShape::Glob));
}

#[test]
fn path_shape_rejects_extensionless_tokens() {
    let repo = known(&[]);
    for token in [
        "src/directory",
        "file",
        "line/col",
        "Apache-2.0/MIT",
        "TS/TSX/JS/Py/Go",
        "pulldown-cmark/pulldown-cmark",
    ] {
        assert_eq!(shape(token, &repo), None, "{token}");
    }
}

#[test]
fn path_shape_requires_a_lettered_stem() {
    let repo = known(&[]);
    for token in ["1.x", "0.13.4", "v1.1", "Apache-2.0"] {
        assert_eq!(shape(token, &repo), None, "{token}");
    }
    assert_eq!(shape("main.c", &repo), Some(PathShape::File));
    assert_eq!(shape("x.rs", &repo), Some(PathShape::File));
}

#[test]
fn path_shape_rejects_all_single_char_pieces() {
    let repo = known(&[]);
    for token in ["e.g", "i.e", "a.k.a", "p.s"] {
        assert_eq!(shape(token, &repo), None, "{token}");
    }
    assert_eq!(shape("a.k.md", &repo), Some(PathShape::File));
}

#[test]
fn path_shape_uses_linguist_then_repo_extensions() {
    assert_eq!(shape("foo.zzq", &known(&[])), None);
    assert_eq!(shape("foo.zzq", &known(&["zzq"])), Some(PathShape::File));
    assert_eq!(shape("README.TXT", &known(&[])), Some(PathShape::File));
    // `com` is a Linguist extension (DCL), so domains stay candidates; the author ignores them.
    assert_eq!(shape("website.com", &known(&[])), Some(PathShape::File));
}

#[test]
fn bare_dotfiles_are_not_candidates_but_dot_directories_are() {
    let repo = known(&[]);
    assert_eq!(shape(".env", &repo), None);
    assert_eq!(shape(".env", &repo), None);
    assert_eq!(
        shape(".github/workflows/ci.yml", &repo),
        Some(PathShape::File)
    );
}

#[test]
fn path_tokens_keep_directory_tails_and_trim_sentence_punctuation() {
    let repo = known(&[]);
    let text = "in docs/guide.md, docs/, and src/*. Then src/**: done.";
    assert_eq!(
        prose_tokens(text, &repo),
        vec![
            ("docs/guide.md".to_owned(), PathShape::File),
            ("docs/".to_owned(), PathShape::Directory),
            ("src/*".to_owned(), PathShape::Glob),
            ("src/**".to_owned(), PathShape::Glob),
        ]
    );
    let spans: Vec<&str> = path_tokens(text, KnownExtensions::new(&repo))
        .map(|token| &text[token.span.start..token.span.end])
        .collect();
    assert_eq!(spans, vec!["docs/guide.md", "docs/", "src/*", "src/**"]);
}

#[test]
fn path_tokens_skip_glob_patterns() {
    let repo = known(&[]);
    for text in [
        "*.generated.ts",
        "**/*.md",
        "src/*.rs",
        "src/**/*.rs",
        "src/*x",
    ] {
        assert!(prose_tokens(text, &repo).is_empty(), "{text}");
    }
}

#[test]
fn path_tokens_keep_emphasised_paths() {
    let repo = known(&[]);
    assert_eq!(
        prose_tokens("read *docs/guide.md* first", &repo),
        vec![("docs/guide.md".to_owned(), PathShape::File)]
    );
}

#[test]
fn path_tokens_skip_url_paths_and_glued_tails() {
    let repo = known(&[]);
    assert!(prose_tokens("see https://example.com/docs/guide.md", &repo).is_empty());
    assert!(prose_tokens("v1/2 and 24/7", &repo).is_empty());
    assert!(prose_tokens("@ref[docs/guide.md]", &repo).is_empty());
}

#[test]
fn code_span_token_requires_the_whole_span_to_be_one_path() {
    let repo = known(&[]);
    let span_shape = |content: &str| {
        code_span_token(
            content,
            ByteSpan::new(0, content.len()),
            KnownExtensions::new(&repo),
        )
        .map(|token| token.shape)
    };
    assert_eq!(
        span_shape("`docs/research/`"),
        Some(Shape::Path(PathShape::Directory))
    );
    assert_eq!(span_shape("`src/*`"), Some(Shape::Path(PathShape::Glob)));
    assert_eq!(span_shape("`x.rs`"), Some(Shape::Path(PathShape::File)));
    assert_eq!(span_shape("`tests/fixtures/<case>/`"), None);
    assert_eq!(span_shape("`*.generated.ts`"), None);
    assert_eq!(span_shape("`My Notes.md`"), None);
    assert_eq!(span_shape("`e.g`"), None);
    assert_eq!(span_shape("`run_check`"), None);
    assert_eq!(span_shape("`HashMap`"), None);
    assert_eq!(span_shape("``"), None);
}

#[test]
fn linguist_table_is_sorted_unique_and_lowercase() {
    let table = super::super::linguist::LINGUIST_EXTENSIONS;
    assert!(table.is_sorted());
    assert!(table.windows(2).all(|pair| pair[0] != pair[1]));
    assert!(table.iter().all(|entry| {
        !entry.is_empty()
            && entry.chars().all(|c| {
                c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '+' | '-')
            })
    }));
    for extension in ["rs", "md", "toml", "txt", "yml", "go", "py"] {
        assert!(is_linguist_extension(extension), "{extension}");
    }
    for extension in ["", "RS", "1", "lock", "rs.in"] {
        assert!(!is_linguist_extension(extension), "{extension}");
    }
}
