use std::fs;

use camino::Utf8PathBuf;

use super::*;
use crate::config;

struct Fixture {
    _dir: tempfile::TempDir,
    root_dir: Utf8PathBuf,
}

impl Fixture {
    fn new(files: &[(&str, &str)]) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root_dir = Utf8PathBuf::from_path_buf(dir.path().join("repo")).unwrap();
        for (path, contents) in files {
            let full = root_dir.join(path);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, contents).unwrap();
        }
        Self {
            _dir: dir,
            root_dir,
        }
    }

    fn coverage(&self) -> CoverageReport {
        let workspace = Workspace::load(config::discover(&self.root_dir).unwrap()).unwrap();
        coverage(&workspace, &[])
    }
}

/// Every site in file order, so tests read like the fixture text.
fn sites(report: &CoverageReport) -> Vec<(&CandidateKind, &CandidateSite)> {
    let mut sites: Vec<(&CandidateKind, &CandidateSite)> = report
        .candidates
        .iter()
        .flat_map(|group| group.sites.iter().map(move |site| (&group.kind, site)))
        .collect();
    sites.sort_by(|a, b| a.1.site.cmp(&b.1.site));
    sites
}

fn describe(report: &CoverageReport) -> Vec<(String, String, String)> {
    sites(report)
        .into_iter()
        .map(|(kind, c)| {
            let kind = match kind {
                CandidateKind::Proposal { replacement } => format!("propose {replacement}"),
                CandidateKind::Unresolvable { .. } => "unresolvable".to_owned(),
                CandidateKind::UnusedAlias { .. } => "unused alias".to_owned(),
                CandidateKind::UnusedIgnore { .. } => "unused ignore".to_owned(),
            };
            (c.site.path.to_string(), c.text.clone(), kind)
        })
        .collect()
}

fn row(path: &str, text: &str, kind: &str) -> (String, String, String) {
    (path.to_owned(), text.to_owned(), kind.to_owned())
}

#[test]
fn paths_in_prose_and_code_spans_are_proposed_when_they_resolve() {
    let fixture = Fixture::new(&[
        (
            "README.md",
            "See `docs/guide.md`, docs/guide.md, and docs/missing.md. Config in anchr.toml.\nAlready @ref[docs/guide.md]. Not https://example.com/docs/guide.md nor v1/2.\n",
        ),
        ("docs/guide.md", "# Guide\n"),
        ("anchr.toml", ""),
    ]);
    let report = fixture.coverage();
    assert_eq!(
        describe(&report),
        vec![
            row(
                "README.md",
                "`docs/guide.md`",
                "propose @ref[docs/guide.md]"
            ),
            row("README.md", "docs/guide.md", "propose @ref[docs/guide.md]"),
            row("README.md", "docs/missing.md", "unresolvable"),
            row("README.md", "anchr.toml", "propose @ref[anchr.toml]"),
        ]
    );
    assert_eq!(report.summary.annotated_refs, 1);
    assert_eq!(report.summary.proposals, 3);
    assert_eq!(report.summary.unresolvable, 1);
    assert_eq!(report.summary.total(), 5);
}

#[test]
fn directory_and_glob_tokens_propose_the_directory_form() {
    let fixture = Fixture::new(&[
        (
            "README.md",
            "See `docs/research/`, src/* and src/**. Also the missing/ directory.\n",
        ),
        ("docs/research/survey.md", ""),
        ("src/lib.rs", ""),
    ]);
    let report = fixture.coverage();
    assert_eq!(
        describe(&report),
        vec![
            row(
                "README.md",
                "`docs/research/`",
                "propose @ref[docs/research/]"
            ),
            row("README.md", "src/*", "propose @ref[src/]"),
            row("README.md", "src/**", "propose @ref[src/]"),
            row("README.md", "missing/", "unresolvable"),
        ]
    );
    let edits = report.proposals();
    let readme = &edits[&FilePath::new(Utf8PathBuf::from("README.md")).unwrap()];
    let described: Vec<(&str, &str)> = readme
        .iter()
        .map(|edit| (edit.expected.as_str(), edit.replacement.as_str()))
        .collect();
    assert_eq!(
        described,
        vec![
            ("`docs/research/`", "@ref[docs/research/]"),
            ("src/*", "@ref[src/]"),
            ("src/**", "@ref[src/]"),
        ]
    );
}

#[test]
fn prose_slash_pairs_and_version_numbers_are_not_candidates() {
    let fixture = Fixture::new(&[(
        "README.md",
        "Reports line/col, licensed Apache-2.0/MIT, e.g. since 1.x, i.e. a.k.a. v1.1;\nsee src/directory and the website, or `struct/class` and `e.g`.\n",
    )]);
    let report = fixture.coverage();
    assert!(report.candidates.is_empty(), "{:?}", describe(&report));
    assert_eq!(report.summary.total(), 0);
}

#[test]
fn repo_extensions_extend_the_linguist_table() {
    let with_file = Fixture::new(&[
        ("README.md", "Read other.zzq first.\n"),
        ("data/notes.zzq", ""),
    ]);
    assert_eq!(
        describe(&with_file.coverage()),
        vec![row("README.md", "other.zzq", "unresolvable")]
    );

    let without_file = Fixture::new(&[("README.md", "Read other.zzq first.\n")]);
    assert!(without_file.coverage().candidates.is_empty());
}

#[test]
fn linguist_extensions_are_known_without_a_repo_file() {
    let fixture = Fixture::new(&[("README.md", "The notes moved from old.txt.\n")]);
    assert_eq!(
        describe(&fixture.coverage()),
        vec![row("README.md", "old.txt", "unresolvable")]
    );
}

#[test]
fn backticked_identifiers_are_never_candidates() {
    let fixture = Fixture::new(&[
        (
            "docs/a.md",
            "Call `validate_token`, then `shared_name`; see `src/auth.rs#validate_token`.\n",
        ),
        (
            "src/auth.rs",
            "pub fn validate_token() {}\npub fn shared_name() {}\n",
        ),
        (
            "src/other.rs",
            "pub fn shared_name() {}\n// see `validate_token`, `shared_name`, and src/auth.rs\n",
        ),
    ]);
    let report = fixture.coverage();
    assert_eq!(
        describe(&report),
        vec![
            row(
                "docs/a.md",
                "`src/auth.rs#validate_token`",
                "propose @ref[src/auth.rs#validate_token]"
            ),
            row("src/other.rs", "src/auth.rs", "propose @ref[src/auth.rs]"),
        ]
    );
    assert_eq!(report.summary.total(), 2);
}

#[test]
fn alias_words_are_proposed_per_file_and_unused_aliases_are_advisories() {
    let fixture = Fixture::new(&[
        (
            "docs/a.md",
            "@ref[docs/guide.md as Guide] @ref[docs/guide.md as Spare]\nRead the Guide, or `Guide`; not [Guide](docs/guide.md), Guidebook, or guide. @[Guide] is done.\n",
        ),
        ("docs/b.md", "Guide is just a word here; so is `Guide`.\n"),
        ("docs/guide.md", "# Guide\n"),
        (
            "src/x.rs",
            "// The Guide struct: `Guide` and Guide\npub struct Guide;\n",
        ),
    ]);
    let report = fixture.coverage();
    let described = describe(&report);
    assert_eq!(
        described
            .iter()
            .filter(|(path, _, kind)| path == "docs/a.md" && kind == "propose @[Guide]")
            .map(|(_, text, _)| text.as_str())
            .collect::<Vec<_>>(),
        vec!["Guide", "`Guide`"]
    );
    assert_eq!(
        described
            .iter()
            .filter(|(path, _, _)| path == "docs/a.md")
            .filter(|(_, _, kind)| kind == "unused alias")
            .map(|(_, text, _)| text.as_str())
            .collect::<Vec<_>>(),
        vec!["Spare"]
    );
    assert!(
        described
            .iter()
            .filter(|(path, _, _)| path != "docs/a.md")
            .all(|(_, _, kind)| !kind.contains("@[")),
        "{described:?}"
    );
    assert_eq!(report.summary.annotated_refs, 3);
    assert_eq!(report.summary.unused_aliases, 1);
    assert_eq!(
        report.summary.total(),
        report.summary.annotated_refs + report.summary.proposals + report.summary.unresolvable
    );
    let edits = report.proposals();
    let a = &edits[&FilePath::new(Utf8PathBuf::from("docs/a.md")).unwrap()];
    assert!(a.iter().all(|edit| edit.replacement == "@[Guide]"));
    assert_eq!(a.len(), 2);
}

#[test]
fn links_and_fences_are_never_candidates_and_proposals_become_edits() {
    let fixture = Fixture::new(&[
        (
            "README.md",
            "[guide](docs/guide.md) and\n\n```\ndocs/guide.md\n```\n\nplus `docs/guide.md`.\n",
        ),
        ("docs/guide.md", ""),
    ]);
    let report = fixture.coverage();
    assert_eq!(report.candidates.len(), 1);
    let edits = report.proposals();
    let readme = &edits[&FilePath::new(Utf8PathBuf::from("README.md")).unwrap()];
    assert_eq!(readme.len(), 1);
    assert_eq!(readme[0].expected, "`docs/guide.md`");
    assert_eq!(readme[0].replacement, "@ref[docs/guide.md]");
}

#[test]
fn noref_lists_suppress_every_token_shape_in_their_file_only() {
    // `src/**` in the first file claims the subtree; the bare `src/` in the second claims
    // only that token.
    let fixture = Fixture::new(&[
        (
            "docs/a.md",
            "@noref[docs/guide.md, src/**, Guide]\n@ref[docs/guide.md as Guide]\nSee `docs/guide.md`, src/x.rs, `src/x.rs#run`, src/*, src/, and Guide (@[Guide]).\n",
        ),
        (
            "docs/b.md",
            "@noref[src/]\nSee `docs/guide.md`, src/x.rs, and src/.\n",
        ),
        ("docs/guide.md", "# Guide\n"),
        ("src/x.rs", "pub fn run() {}\n"),
    ]);
    let report = fixture.coverage();
    assert_eq!(
        describe(&report),
        vec![
            row(
                "docs/b.md",
                "`docs/guide.md`",
                "propose @ref[docs/guide.md]"
            ),
            row("docs/b.md", "src/x.rs", "propose @ref[src/x.rs]"),
        ]
    );
    assert_eq!(report.summary.ignored, 7);
    assert_eq!(report.summary.unused_ignores, 0);
    assert_eq!(report.summary.annotated_refs, 2);
    assert_eq!(report.summary.total(), 4);
    assert!(report.unused_config_ignores.is_empty());
}

#[test]
fn unused_and_duplicate_noref_entries_are_advisories_at_their_span() {
    let source = "@noref[foo.ts, bar.md, foo.ts]\nMentions foo.ts only.\n";
    let fixture = Fixture::new(&[("docs/a.md", source)]);
    let report = fixture.coverage();
    assert_eq!(
        describe(&report),
        vec![
            row("docs/a.md", "bar.md", "unused ignore"),
            row("docs/a.md", "foo.ts", "unused ignore"),
        ]
    );
    let spans: Vec<&str> = sites(&report)
        .iter()
        .map(|(_, c)| &source[c.site.span.start..c.site.span.end])
        .collect();
    assert_eq!(spans, vec!["bar.md", "foo.ts"]);
    assert_eq!(sites(&report)[1].1.site.span.start, 23);
    assert_eq!(report.summary.ignored, 1);
    assert_eq!(report.summary.unused_ignores, 2);
    assert!(report.proposals().is_empty());
}

#[test]
fn config_tokens_apply_across_files_and_report_unused_entries() {
    let fixture = Fixture::new(&[
        (
            "anchr.toml",
            "[ignore]\ntokens = [\"CLAUDE.md\", \"never.md\", \"**/AGENTS.md\"]\n",
        ),
        (
            "docs/a.md",
            "See `CLAUDE.md` and `docs/guide.md`. @ref[docs/guide.md]\n",
        ),
        (
            "docs/b.md",
            "See `docs/guide.md`, CLAUDE.md, and docs/AGENTS.md. @ref[docs/guide.md]\n",
        ),
        ("docs/guide.md", "# Guide\n"),
    ]);
    let workspace = Workspace::load(config::discover(&fixture.root_dir).unwrap()).unwrap();
    let report = coverage(&workspace, &[]);
    assert_eq!(
        describe(&report),
        vec![
            row(
                "docs/a.md",
                "`docs/guide.md`",
                "propose @ref[docs/guide.md]"
            ),
            row(
                "docs/b.md",
                "`docs/guide.md`",
                "propose @ref[docs/guide.md]"
            ),
        ]
    );
    assert_eq!(
        report
            .unused_config_ignores
            .iter()
            .map(NoRefEntry::as_str)
            .collect::<Vec<_>>(),
        vec!["never.md"]
    );
    assert_eq!(report.summary.ignored, 3);
    assert_eq!(report.summary.unused_ignores, 1);
    assert_eq!(report.summary.annotated_refs, 2);
    assert_eq!(report.summary.total(), 4);

    let only_a = FilePath::new(Utf8PathBuf::from("docs/a.md")).unwrap();
    let narrowed = coverage(&workspace, &[only_a]);
    assert_eq!(narrowed.candidates.len(), 1);
    assert!(
        narrowed.unused_config_ignores.is_empty(),
        "a narrowed run cannot judge root-wide entries"
    );
    assert_eq!(narrowed.summary.unused_ignores, 0);
}

#[test]
fn one_token_with_one_verdict_is_one_group_and_groups_have_a_fixed_order() {
    let fixture = Fixture::new(&[
        (
            "docs/a.md",
            "@ref[docs/guide.md as Guide] @ref[docs/guide.md as Spare]\nSee gen.js, `gen.js`, and `docs/guide.md`; the Guide and `Guide` (@[Guide]).\n",
        ),
        ("docs/b.md", "Header from gen.js.\n"),
        ("docs/guide.md", "# Guide\n"),
        ("src/x.rs", "// Generated by gen.js\n"),
    ]);
    let report = fixture.coverage();
    let described: Vec<(&str, &str, usize)> = report
        .candidates
        .iter()
        .map(|group| {
            let kind = match &group.kind {
                CandidateKind::Proposal { .. } => "proposal",
                CandidateKind::Unresolvable { .. } => "unresolvable",
                CandidateKind::UnusedAlias { .. } => "unused alias",
                CandidateKind::UnusedIgnore { .. } => "unused ignore",
            };
            (kind, group.token.as_str(), group.sites.len())
        })
        .collect();
    assert_eq!(
        described,
        vec![
            ("proposal", "Guide", 2),
            ("proposal", "docs/guide.md", 1),
            ("unresolvable", "gen.js", 4),
            ("unused alias", "Spare", 1),
        ]
    );
    let generated = &report.candidates[2];
    let paths: Vec<String> = generated
        .sites
        .iter()
        .map(|c| c.site.path.to_string())
        .collect();
    assert_eq!(
        paths,
        vec!["docs/a.md", "docs/a.md", "docs/b.md", "src/x.rs"]
    );
    let texts: Vec<&str> = generated.sites.iter().map(|c| c.text.as_str()).collect();
    assert_eq!(texts, vec!["gen.js", "`gen.js`", "gen.js", "gen.js"]);
    assert_eq!(report.summary.unresolvable, 4);
    assert_eq!(report.summary.proposals, 3);
}

#[test]
fn bare_tokens_that_resolve_beside_their_file_are_proposed_with_dot_slash() {
    let fixture = Fixture::new(&[
        (
            "docs/a.md",
            "See guide.md, `sub/x.md`, x.rs#run and sub/*.\n",
        ),
        ("docs/guide.md", "# Guide\n"),
        ("docs/sub/x.md", ""),
        ("docs/x.rs", "pub fn run() {}\n"),
    ]);
    assert_eq!(
        describe(&fixture.coverage()),
        vec![
            row("docs/a.md", "guide.md", "propose @ref[./guide.md]"),
            row("docs/a.md", "`sub/x.md`", "propose @ref[./sub/x.md]"),
            row("docs/a.md", "x.rs#run", "propose @ref[./x.rs#run]"),
            row("docs/a.md", "sub/*", "propose @ref[./sub/]"),
        ]
    );
}

#[test]
fn dot_only_tokens_are_prose_about_the_syntax() {
    let fixture = Fixture::new(&[
        (
            "docs/a.md",
            "Write `./` or `../` for relative paths, never `../../`.\n",
        ),
        ("docs/guide.md", ""),
    ]);
    let report = fixture.coverage();
    assert!(report.candidates.is_empty(), "{:?}", describe(&report));
}

#[test]
fn a_token_found_under_exactly_one_ancestor_is_proposed_root_relative() {
    let one = Fixture::new(&[
        ("a/b/c/d.md", "See n/f.md and n/f.md#run.\n"),
        ("a/b/n/f.md", ""),
    ]);
    assert_eq!(
        describe(&one.coverage()),
        vec![
            row("a/b/c/d.md", "n/f.md", "propose @ref[a/b/n/f.md]"),
            row("a/b/c/d.md", "n/f.md#run", "unresolvable"),
        ]
    );

    let two = Fixture::new(&[
        ("a/b/c/d.md", "See n/f.md.\n"),
        ("a/b/n/f.md", ""),
        ("a/n/f.md", ""),
    ]);
    assert_eq!(
        describe(&two.coverage()),
        vec![row("a/b/c/d.md", "n/f.md", "unresolvable")]
    );
}

#[test]
fn an_unresolvable_token_with_a_unique_basename_names_where_it_is() {
    let unique = Fixture::new(&[
        ("docs/a.md", "See missing/guide.md.\n"),
        ("elsewhere/deep/guide.md", ""),
    ]);
    let report = unique.coverage();
    let CandidateKind::Unresolvable { reason } = &report.candidates[0].kind else {
        panic!("{:?}", describe(&report));
    };
    assert!(
        reason.ends_with("; one file named `guide.md` exists at `elsewhere/deep/guide.md`"),
        "{reason}"
    );

    let shared = Fixture::new(&[
        ("docs/a.md", "See missing/guide.md.\n"),
        ("elsewhere/deep/guide.md", ""),
        ("other/guide.md", ""),
    ]);
    let report = shared.coverage();
    let CandidateKind::Unresolvable { reason } = &report.candidates[0].kind else {
        panic!("{:?}", describe(&report));
    };
    assert!(!reason.contains("one file named"), "{reason}");
}

#[test]
fn a_local_entry_shadows_a_global_one() {
    let fixture = Fixture::new(&[
        ("anchr.toml", "[ignore]\ntokens = [\"foo.ts\"]\n"),
        ("docs/a.md", "@noref[foo.ts]\nSee foo.ts.\n"),
    ]);
    let report = fixture.coverage();
    assert!(report.candidates.is_empty());
    assert_eq!(report.summary.ignored, 1);
    assert_eq!(report.unused_config_ignores.len(), 1);
}
