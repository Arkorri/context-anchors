use std::time::Duration;

use camino::Utf8PathBuf;

use super::*;
use crate::text::{Container, FileAnalyzer, LanguageRegistry};

fn scan(source: &str) -> FileScan {
    let registry = LanguageRegistry::new().unwrap();
    FileAnalyzer::new(&registry, Duration::from_secs(1))
        .scan(Container::Plaintext, source, &file("a.txt"))
        .unwrap()
}

fn file(path: &str) -> FilePath {
    FilePath::new(Utf8PathBuf::from(path)).unwrap()
}

fn id(raw: &str) -> AnchorId {
    AnchorId::parse(raw).unwrap()
}

#[test]
fn aliases_bind_to_the_first_declaration_and_uses_join_backrefs() {
    let index = index_with(&[(
        "a.txt",
        "@anchor[x] @ref[#x as X] @[X] @[X] @[Y] @ref[#x as X] @ref[#x as Unused]",
    )]);
    let record = index.file_record(&file("a.txt")).unwrap();
    let x = Alias::parse("X").unwrap();
    assert_eq!(record.aliases.binding(&x).unwrap().declaration.start, 11);
    let duplicates: Vec<(&Alias, &[ByteSpan])> = record.aliases.duplicates().collect();
    assert_eq!(duplicates.len(), 1);
    assert_eq!(duplicates[0].1.len(), 2);

    let uses: Vec<AliasUse<'_>> = index.alias_uses().collect();
    assert_eq!(uses.len(), 3);
    assert_eq!(
        uses.iter()
            .filter(|use_site| use_site.binding.is_some())
            .count(),
        2
    );
    assert_eq!(index.alias_use_count(&file("a.txt"), &x), 2);

    let target = crate::marker::parse_target("#x", None).unwrap().target;
    let backrefs: Vec<RefSite<'_>> = index.backrefs(&target).collect();
    assert_eq!(backrefs.len(), 5);
    assert_eq!(backrefs.iter().filter(|r| r.via.is_some()).count(), 2);
    assert_eq!(backrefs.iter().filter(|r| r.declares.is_some()).count(), 3);

    let unused: Vec<(&FilePath, &Alias, &AliasBinding)> = index.unused_aliases().collect();
    assert_eq!(unused.len(), 1);
    assert_eq!(unused[0].1.as_str(), "Unused");
}

fn index_with(files: &[(&str, &str)]) -> Index {
    let mut index = Index::new(RootName::parse("r").unwrap());
    for (path, source) in files {
        index.update_file(file(path), scan(source));
    }
    index
}

#[test]
fn anchors_refs_and_malformed_are_indexed_per_file() {
    let index = index_with(&[
        ("a.md", "@anchor[a] @ref[#b] @ref["),
        ("b.md", "@anchor[b] @ref[#a] @ref[a.md]"),
    ]);
    assert_eq!(index.file_count(), 2);
    assert_eq!(index.anchor_count(), 2);
    assert_eq!(index.anchor_sites(&id("a")).len(), 1);
    assert_eq!(index.anchor_sites(&id("a"))[0].path, file("a.md"));
    assert_eq!(index.anchor_sites(&id("zzz")).len(), 0);
    assert_eq!(index.refs().count(), 3);
    assert_eq!(index.malformed().count(), 1);
    assert_eq!(index.duplicate_anchors().count(), 0);
}

#[test]
fn duplicates_are_reported_with_sorted_sites() {
    let index = index_with(&[
        ("b.md", "@anchor[dup]"),
        ("a.md", "@anchor[dup] @anchor[dup]"),
    ]);
    let duplicates: Vec<_> = index.duplicate_anchors().collect();
    assert_eq!(duplicates.len(), 1);
    let (found, sites) = &duplicates[0];
    assert_eq!(**found, id("dup"));
    let paths: Vec<&str> = sites.iter().map(|s| s.path.as_str()).collect();
    assert_eq!(paths, vec!["a.md", "a.md", "b.md"]);
}

#[test]
fn updating_and_removing_a_file_keeps_the_anchor_map_consistent() {
    let mut index = index_with(&[
        ("a.md", "@anchor[a] @anchor[shared]"),
        ("b.md", "@anchor[shared]"),
    ]);
    assert_eq!(index.anchor_sites(&id("shared")).len(), 2);

    index.update_file(file("a.md"), scan("@anchor[renamed]"));
    assert!(index.anchor_sites(&id("a")).is_empty());
    assert_eq!(index.anchor_sites(&id("renamed")).len(), 1);
    assert_eq!(index.anchor_sites(&id("shared")).len(), 1);
    assert_eq!(index.anchor_sites(&id("shared"))[0].path, file("b.md"));

    index.remove_file(&file("b.md"));
    assert!(index.anchor_sites(&id("shared")).is_empty());
    assert_eq!(index.anchor_ids().count(), 1);
    assert_eq!(index.file_count(), 1);

    index.remove_file(&file("never-there.md"));
    assert_eq!(index.file_count(), 1);
}

#[test]
fn backrefs_match_the_target_with_the_root_made_explicit() {
    let index = index_with(&[
        ("a.md", "@ref[#x] @ref[other:#x] @ref[#y] @ref[r:#x]"),
        ("b.md", "@ref[#x]"),
    ]);
    let target = crate::marker::parse_target("#x", None).unwrap().target;
    let mut paths: Vec<String> = index
        .backrefs(&target)
        .map(|r| r.site.path.to_string())
        .collect();
    paths.sort_unstable();
    assert_eq!(paths, vec!["a.md", "a.md", "b.md"]);
    assert!(index.backrefs(&target).all(|r| r.id_span.is_some()));

    let prefixed = crate::marker::parse_target("r:#x", None).unwrap().target;
    assert_eq!(index.backrefs(&prefixed).count(), 3);
    let other = crate::marker::parse_target("other:#x", None)
        .unwrap()
        .target;
    assert_eq!(index.backrefs(&other).count(), 1);
}

#[test]
fn anchors_expose_their_id_spans() {
    let index = index_with(&[("a.md", "see @anchor[auth/flow] here")]);
    let anchors: Vec<_> = index.anchors().collect();
    assert_eq!(anchors.len(), 1);
    assert_eq!(anchors[0].id.as_str(), "auth/flow");
    assert_eq!(anchors[0].id_span, ByteSpan::new(12, 21));
    assert_eq!(anchors[0].site.span, ByteSpan::new(4, 22));
}
