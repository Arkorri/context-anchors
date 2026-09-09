//! Per-root index of markers. Derived, in memory, never authoritative: `grep '@anchor\[id\]'`
//! finds the same thing.

use std::collections::HashMap;
use std::collections::hash_map::Entry;

use crate::marker::{Alias, AnchorId, MalformedMarker, Marker, MarkerPayload, RefTarget};
use crate::root::{FilePath, RootName};
use crate::scan::ScannedFile;
use crate::span::{ByteSpan, LineIndex};
use crate::text::{FileScan, RegionKind};
use crate::tree::FileTree;

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
    pub aliases: AliasTable,
}

/// The aliases one file declares, built from that file's markers alone. The first declaration
/// of a name wins the binding; every later one is a duplicate.
#[derive(Debug, Clone, Default)]
pub struct AliasTable {
    bindings: HashMap<Alias, AliasBinding>,
    duplicates: HashMap<Alias, Vec<ByteSpan>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AliasBinding {
    pub target: RefTarget,
    /// The whole declaring marker.
    pub declaration: ByteSpan,
    /// The alias token inside it, which an alias rename rewrites.
    pub alias_span: ByteSpan,
}

impl AliasTable {
    fn from_markers(markers: &[Marker]) -> Self {
        let mut table = Self::default();
        for marker in markers {
            let MarkerPayload::Ref {
                target,
                alias: Some(declared),
                ..
            } = &marker.payload
            else {
                continue;
            };
            match table.bindings.entry(declared.alias.clone()) {
                Entry::Vacant(slot) => {
                    slot.insert(AliasBinding {
                        target: target.clone(),
                        declaration: marker.span,
                        alias_span: declared.span,
                    });
                }
                Entry::Occupied(first) => {
                    table
                        .duplicates
                        .entry(declared.alias.clone())
                        .or_insert_with(|| vec![first.get().declaration])
                        .push(marker.span);
                }
            }
        }
        table
    }

    pub fn binding(&self, alias: &Alias) -> Option<&AliasBinding> {
        self.bindings.get(alias)
    }

    pub fn aliases(&self) -> impl Iterator<Item = &Alias> {
        self.bindings.keys()
    }

    /// Names declared more than once, with every declaring marker's span in file order.
    pub fn duplicates(&self) -> impl Iterator<Item = (&Alias, &[ByteSpan])> {
        self.duplicates
            .iter()
            .map(|(alias, spans)| (alias, spans.as_slice()))
    }
}

/// A reference and where it was written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefSite<'a> {
    pub target: &'a RefTarget,
    /// Bytes a rename of the referenced anchor rewrites, when the target is an anchor.
    pub id_span: Option<ByteSpan>,
    /// The alias this reference declares, when written `target as Alias`.
    pub declares: Option<&'a Alias>,
    /// Set when this site is an `@[Alias]` use reached through its declaration.
    pub via: Option<&'a Alias>,
    pub site: Site,
}

/// An `@[Alias]` and, when its file declares the alias, what it is bound to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AliasUse<'a> {
    pub alias: &'a Alias,
    pub binding: Option<&'a AliasBinding>,
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
/// every update so the two cannot drift. `tree` is every path the scan enumerated, markers or
/// not: the scan is the authority on what exists, and the tree is its result.
#[derive(Debug, Clone)]
pub struct Index {
    root: RootName,
    files: HashMap<FilePath, FileRecord>,
    anchors_by_id: HashMap<AnchorId, Vec<Site>>,
    tree: FileTree,
}

impl Index {
    pub fn new(root: RootName) -> Self {
        Self {
            root,
            files: HashMap::new(),
            anchors_by_id: HashMap::new(),
            tree: FileTree::default(),
        }
    }

    pub fn from_scan(root: RootName, files: Vec<ScannedFile>, tree: FileTree) -> Self {
        let mut index = Self::new(root);
        index.tree = tree;
        for file in files {
            index.update_file(file.path, file.scan);
        }
        index
    }

    pub fn root(&self) -> &RootName {
        &self.root
    }

    pub fn tree(&self) -> &FileTree {
        &self.tree
    }

    /// A file that exists but is not lexed: an editor buffer with no container.
    pub fn mark_present(&mut self, path: &FilePath) {
        self.tree.insert(path, crate::tree::Entry::File);
    }

    pub fn mark_absent(&mut self, path: &FilePath) {
        self.tree.remove(path);
    }

    pub fn update_file(&mut self, path: FilePath, scan: FileScan) {
        self.remove_file(&path);
        self.tree.insert(&path, crate::tree::Entry::File);
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
        let aliases = AliasTable::from_markers(&scan.markers);
        self.files.insert(
            path,
            FileRecord {
                markers: scan.markers,
                malformed: scan.malformed,
                line_index: scan.line_index,
                aliases,
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
                        target,
                        id_span,
                        alias,
                    } => Some(RefSite {
                        target,
                        id_span: *id_span,
                        declares: alias.as_ref().map(|declared| &declared.alias),
                        via: None,
                        site: site(root, path, marker.span, marker.region),
                    }),
                    MarkerPayload::Anchor { .. }
                    | MarkerPayload::Use { .. }
                    | MarkerPayload::NoRef { .. } => None,
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
                    MarkerPayload::Ref { .. }
                    | MarkerPayload::Use { .. }
                    | MarkerPayload::NoRef { .. } => None,
                })
        })
    }

    /// Every `@[Alias]` in this root, bound when its file declares the alias.
    pub fn alias_uses(&self) -> impl Iterator<Item = AliasUse<'_>> {
        let root = &self.root;
        self.files.iter().flat_map(move |(path, record)| {
            record
                .markers
                .iter()
                .filter_map(move |marker| match &marker.payload {
                    MarkerPayload::Use { alias } => Some(AliasUse {
                        alias,
                        binding: record.aliases.binding(alias),
                        site: site(root, path, marker.span, marker.region),
                    }),
                    MarkerPayload::Anchor { .. }
                    | MarkerPayload::Ref { .. }
                    | MarkerPayload::NoRef { .. } => None,
                })
        })
    }

    pub fn alias_use_count(&self, path: &FilePath, alias: &Alias) -> usize {
        self.files.get(path).map_or(0, |record| {
            record
                .markers
                .iter()
                .filter(|marker| {
                    matches!(&marker.payload, MarkerPayload::Use { alias: used } if used == alias)
                })
                .count()
        })
    }

    /// Aliases declared more than once in one file, with every declaration site in file order.
    pub fn duplicate_aliases(&self) -> impl Iterator<Item = (&FilePath, &Alias, Vec<Site>)> {
        let root = &self.root;
        self.files.iter().flat_map(move |(path, record)| {
            record.aliases.duplicates().map(move |(alias, spans)| {
                let sites = spans
                    .iter()
                    .map(|span| site(root, path, *span, region_of(record, *span)))
                    .collect();
                (path, alias, sites)
            })
        })
    }

    /// Aliases a file declares and never uses.
    pub fn unused_aliases(&self) -> impl Iterator<Item = (&FilePath, &Alias, &AliasBinding)> {
        self.files.iter().flat_map(move |(path, record)| {
            record
                .aliases
                .bindings
                .iter()
                .filter(move |(alias, _)| self.alias_use_count(path, alias) == 0)
                .map(move |(alias, binding)| (path, alias, binding))
        })
    }

    /// Every reference to `target`, treating a bare target and one prefixed with this
    /// root's own name as the same thing. Alias uses count through their declaration.
    pub fn backrefs<'a>(&'a self, target: &RefTarget) -> impl Iterator<Item = RefSite<'a>> {
        let wanted = target.resolved_in(&self.root);
        let wanted_by_use = wanted.clone();
        let direct = self
            .refs()
            .filter(move |reference| reference.target.resolved_in(&self.root) == wanted);
        let through_aliases = self.alias_uses().filter_map(move |use_site| {
            let binding = use_site.binding?;
            (binding.target.resolved_in(&self.root) == wanted_by_use).then_some(RefSite {
                target: &binding.target,
                id_span: None,
                declares: None,
                via: Some(use_site.alias),
                site: use_site.site,
            })
        });
        direct.chain(through_aliases)
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

/// The region of the marker at `span`; declarations always come from a lexed marker.
fn region_of(record: &FileRecord, span: ByteSpan) -> RegionKind {
    record
        .markers
        .iter()
        .find(|marker| marker.span == span)
        .map_or(RegionKind::Prose, |marker| marker.region)
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
mod tests;
