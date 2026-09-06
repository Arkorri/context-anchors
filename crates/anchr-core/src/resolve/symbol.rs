use std::collections::HashMap;
use std::fs;

use camino::Utf8Path;

use super::Unverified;
use crate::marker::RelPath;
use crate::root::{Root, RootName};
use crate::text::{AnalyzeError, FileAnalyzer, SymbolTable};

/// A file's declarations plus which grammar produced them.
pub(super) struct FileSymbols {
    table: SymbolTable,
    pub(super) language: &'static str,
}

impl FileSymbols {
    pub(super) fn contains(&self, name: &crate::marker::SymbolName) -> bool {
        self.table.contains(name)
    }

    pub(super) fn names(&self) -> impl Iterator<Item = &str> {
        self.table.names()
    }
}

impl std::ops::Deref for FileSymbols {
    type Target = SymbolTable;

    fn deref(&self) -> &SymbolTable {
        &self.table
    }
}

/// Parses each referenced file once per run, whatever the outcome.
#[derive(Default)]
pub(super) struct SymbolCache {
    files: HashMap<(RootName, RelPath), Result<FileSymbols, Unverified>>,
}

impl SymbolCache {
    pub(super) fn load(
        &mut self,
        root: &Root,
        path: &RelPath,
        absolute: &Utf8Path,
        analyzer: &mut FileAnalyzer<'_>,
    ) -> Result<&FileSymbols, Unverified> {
        let key = (root.name.clone(), path.clone());
        if !self.files.contains_key(&key) {
            let loaded = load(root, path, absolute, analyzer);
            self.files.insert(key.clone(), loaded);
        }
        match self.files.get(&key) {
            Some(Ok(symbols)) => Ok(symbols),
            Some(Err(unverified)) => Err(unverified.clone()),
            // Inserted just above.
            None => unreachable!("symbol cache entry was just inserted"),
        }
    }

    pub(super) fn cached(&self, root: &RootName, path: &RelPath) -> Option<&FileSymbols> {
        self.files
            .get(&(root.clone(), path.clone()))
            .and_then(|loaded| loaded.as_ref().ok())
    }
}

fn load(
    root: &Root,
    path: &RelPath,
    absolute: &Utf8Path,
    analyzer: &mut FileAnalyzer<'_>,
) -> Result<FileSymbols, Unverified> {
    let name = root.name.clone();
    let extension = path.extension().map(str::to_ascii_lowercase);
    let spec = extension
        .as_deref()
        .and_then(|ext| analyzer.registry().for_extension(ext))
        .ok_or_else(|| Unverified::NoGrammar {
            root: name.clone(),
            path: path.clone(),
            extension: extension.clone(),
        })?;

    let limit = root.config.scan.max_file_bytes;
    let bytes = fs::metadata(absolute)
        .map_err(|error| Unverified::TargetUnreadable {
            root: name.clone(),
            path: path.clone(),
            message: error.to_string(),
        })?
        .len();
    if bytes > limit {
        return Err(Unverified::TargetTooLarge {
            root: name,
            path: path.clone(),
            bytes,
            limit,
        });
    }
    let raw = fs::read(absolute).map_err(|error| Unverified::TargetUnreadable {
        root: name.clone(),
        path: path.clone(),
        message: error.to_string(),
    })?;
    let source = String::from_utf8(raw).map_err(|_| Unverified::TargetNotUtf8 {
        root: name.clone(),
        path: path.clone(),
    })?;

    let table = analyzer
        .symbols(spec, &source)
        .map_err(|error| match error {
            AnalyzeError::ParseTimeout { .. } => Unverified::ParseTimeout {
                root: name.clone(),
                path: path.clone(),
            },
            AnalyzeError::SymbolTableTruncated => Unverified::SymbolTableTruncated {
                root: name.clone(),
                path: path.clone(),
            },
            other => Unverified::AnalyzeFailed {
                root: name.clone(),
                path: path.clone(),
                message: other.to_string(),
            },
        })?;
    Ok(FileSymbols {
        table,
        language: spec.name(),
    })
}
