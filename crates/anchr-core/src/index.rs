//! Per-root index of markers. Derived, in memory, never authoritative: `grep '@anchor\[id\]'`
//! finds the same thing.

use std::collections::HashMap;

use crate::marker::{AnchorId, MalformedMarker, Marker, MarkerPayload, RefTarget};
use crate::root::{FilePath, RootName};
use crate::scan::ScannedFile;
use crate::span::{ByteSpan, LineIndex};
use crate::text::{FileScan, RegionKind};

/// Where a marker sits: which root, which file, which bytes.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Site {
    pub root: RootName,
    pub path: FilePath,
    pub span: ByteSpan,
    pub region: RegionKind,
}

#[derive(Debug, Clone)]
pub struct FileRecord {
    pub markers: Vec<Marker>,
    pub malformed: Vec<MalformedMarker>,
    pub line_index: LineIndex,
}

/// A reference and where it was written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefSite<'a> {
    pub target: &'a RefTarget,
    /// Bytes a rename of the referenced anchor rewrites, when the target is an anchor.
    pub id_span: Option<ByteSpan>,
    pub site: Site,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MalformedSite<'a> {
    pub malformed: &'a MalformedMarker,
    pub site: Site,
}

/// An anchor declaration and where it was written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnchorSite<'a> {
    pub id: &'a AnchorId,
    /// The bytes holding the id, which a rename rewrites.
    pub id_span: ByteSpan,
    pub site: Site,
}

/// `files` is the single owner of marker data; `anchors_by_id` is re-derived for a file on
/// every update so the two cannot drift.
#[derive(Debug, Clone)]
pub struct Index {
    root: RootName,
    files: HashMap<FilePath, FileRecord>,
    anchors_by_id: HashMap<AnchorId, Vec<Site>>,
}

impl Index {
    pub fn new(root: RootName) -> Self {
        Self {
            root,
            files: HashMap::new(),
            anchors_by_id: HashMap::new(),
        }
    }

    pub fn from_scan(root: RootName, files: Vec<ScannedFile>) -> Self {
        let mut index = Self::new(root);
        for file in files {
            index.update_file(file.path, file.scan);
        }
        index
    }

    pub fn root(&self) -> &RootName {
        &self.root
    }

    pub fn update_file(&mut self, path: FilePath, scan: FileScan) {
        self.remove_file(&path);
        for marker in &scan.markers {
            if let MarkerPayload::Anchor { id } = &marker.payload {
                self.anchors_by_id.entry(id.clone()).or_default().push(site(
                    &self.root,
                    &path,
                    marker.span,
                    marker.region,
                ));
            }
        }
        self.files.insert(
            path,
            FileRecord {
                markers: scan.markers,
                malformed: scan.malformed,
                line_index: scan.line_index,
            },
        );
    }

    pub fn remove_file(&mut self, path: &FilePath) {
        let Some(record) = self.files.remove(path) else {
            return;
        };
        for marker in &record.markers {
            if let MarkerPayload::Anchor { id } = &marker.payload
                && let Some(sites) = self.anchors_by_id.get_mut(id)
            {
                sites.retain(|site| site.path != *path);
                if sites.is_empty() {
                    self.anchors_by_id.remove(id);
                }
            }
        }
    }

    pub fn anchor_sites(&self, id: &AnchorId) -> &[Site] {
        self.anchors_by_id.get(id).map_or(&[], Vec::as_slice)
    }

    pub fn anchor_ids(&self) -> impl Iterator<Item = &AnchorId> {
        self.anchors_by_id.keys()
    }

    /// Anchor ids declared more than once in this root, with every site, sites sorted.
    pub fn duplicate_anchors(&self) -> impl Iterator<Item = (&AnchorId, Vec<Site>)> {
        self.anchors_by_id
            .iter()
            .filter(|(_, sites)| sites.len() > 1)
            .map(|(id, sites)| {
                let mut sorted = sites.clone();
                sorted.sort();
                (id, sorted)
            })
    }

    pub fn refs(&self) -> impl Iterator<Item = RefSite<'_>> {
        let root = &self.root;
        self.files.iter().flat_map(move |(path, record)| {
            record
                .markers
                .iter()
                .filter_map(move |marker| match &marker.payload {
                    MarkerPayload::Ref {
                        target, id_span, ..
                    } => Some(RefSite {
                        target,
                        id_span: *id_span,
                        site: site(root, path, marker.span, marker.region),
                    }),
                    MarkerPayload::Anchor { .. } | MarkerPayload::Use { .. } => None,
                })
        })
    }

    pub fn malformed(&self) -> impl Iterator<Item = MalformedSite<'_>> {
        let root = &self.root;
        self.files.iter().flat_map(move |(path, record)| {
            record.malformed.iter().map(move |malformed| MalformedSite {
                malformed,
                site: site(root, path, malformed.span, malformed.region),
            })
        })
    }

    pub fn anchors(&self) -> impl Iterator<Item = AnchorSite<'_>> {
        let root = &self.root;
        self.files.iter().flat_map(move |(path, record)| {
            record
                .markers
                .iter()
                .filter_map(move |marker| match &marker.payload {
                    MarkerPayload::Anchor { id } => Some(AnchorSite {
                        id,
                        id_span: marker.body_span,
                        site: site(root, path, marker.span, marker.region),
                    }),
                    MarkerPayload::Ref { .. } | MarkerPayload::Use { .. } => None,
                })
        })
    }

    /// Every reference to `target`, treating a bare target and one prefixed with this
    /// root's own name as the same thing.
    pub fn backrefs<'a>(&'a self, target: &RefTarget) -> impl Iterator<Item = RefSite<'a>> {
        let wanted = target.resolved_in(&self.root);
        self.refs()
            .filter(move |reference| reference.target.resolved_in(&self.root) == wanted)
    }

    pub fn line_index(&self, path: &FilePath) -> Option<&LineIndex> {
        self.files.get(path).map(|record| &record.line_index)
    }

    pub fn file_record(&self, path: &FilePath) -> Option<&FileRecord> {
        self.files.get(path)
    }

    pub fn file_paths(&self) -> impl Iterator<Item = &FilePath> {
        self.files.keys()
    }

    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    pub fn anchor_count(&self) -> usize {
        self.anchors_by_id.values().map(Vec::len).sum()
    }
}

fn site(root: &RootName, path: &FilePath, span: ByteSpan, region: RegionKind) -> Site {
    Site {
        root: root.clone(),
        path: path.clone(),
        span,
        region,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::time::Duration;

    use camino::Utf8PathBuf;

    use super::*;
    use crate::text::{Container, FileAnalyzer, LanguageRegistry};

    fn scan(source: &str) -> FileScan {
        let registry = LanguageRegistry::new().unwrap();
        FileAnalyzer::new(&registry, Duration::from_secs(1))
            .scan(Container::Plaintext, source)
            .unwrap()
    }

    fn file(path: &str) -> FilePath {
        FilePath::new(Utf8PathBuf::from(path)).unwrap()
    }

    fn id(raw: &str) -> AnchorId {
        AnchorId::parse(raw).unwrap()
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
        let target = crate::marker::parse_target("#x").unwrap().target;
        let mut paths: Vec<String> = index
            .backrefs(&target)
            .map(|r| r.site.path.to_string())
            .collect();
        paths.sort_unstable();
        assert_eq!(paths, vec!["a.md", "a.md", "b.md"]);
        assert!(index.backrefs(&target).all(|r| r.id_span.is_some()));

        let prefixed = crate::marker::parse_target("r:#x").unwrap().target;
        assert_eq!(index.backrefs(&prefixed).count(), 3);
        let other = crate::marker::parse_target("other:#x").unwrap().target;
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
}
