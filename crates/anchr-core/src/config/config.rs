//! @ref[anchr.toml]: parsed defensively, then validated once into [`Config`].

use std::collections::BTreeMap;
use std::ops::Range;
use std::time::Duration;

use camino::{Utf8Path, Utf8PathBuf};
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use ignore::Match;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use serde::Deserialize;
use toml::Spanned;

use crate::marker::NoRefEntry;
use crate::root::{RootName, RootNameError};
use crate::text::ContainerRules;

pub const CONFIG_FILE_NAME: &str = "anchr.toml";
pub const DEFAULT_MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
pub const DEFAULT_PARSE_BUDGET: Duration = Duration::from_secs(5);

/// Whether findings the checker could not verify block the run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UnverifiedPolicy {
    #[default]
    Report,
    Error,
}

#[derive(Debug, Clone)]
pub struct ScanConfig {
    /// `None` scans every file with a known container; `Some` additionally requires a match.
    pub include: Option<GlobSet>,
    pub include_patterns: Vec<String>,
    pub max_file_bytes: u64,
    pub parse_budget: Duration,
}

impl Default for ScanConfig {
    fn default() -> Self {
        Self {
            include: None,
            include_patterns: Vec::new(),
            max_file_bytes: DEFAULT_MAX_FILE_BYTES,
            parse_budget: DEFAULT_PARSE_BUDGET,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CheckConfig {
    pub unverified: UnverifiedPolicy,
}

/// The two ignore questions: which paths anchr never looks at, and which prose strings it never
/// proposes as references.
#[derive(Debug, Clone)]
pub struct IgnoreConfig {
    /// gitignore-syntax lines rooted at the root directory, layered on `.gitignore`. The walker
    /// prunes by them and `path_pattern` names the one that removed a path.
    pub paths: Gitignore,
    /// Globs matched against coverage tokens. Read from the current root only.
    pub tokens: Vec<NoRefEntry>,
}

impl IgnoreConfig {
    // @noref[target/**]
    /// The `[ignore] paths` line that removes `path`, as written. A directory also matches a
    /// line written for its contents, so `target/**` is reported for `target` itself.
    pub fn path_pattern(&self, path: &Utf8Path, is_dir: bool) -> Option<&str> {
        let matched = match self.paths.matched(path, is_dir) {
            Match::None if is_dir => self.paths.matched(format!("{path}/"), true),
            matched => matched,
        };
        match matched {
            Match::Ignore(glob) => Some(glob.original()),
            Match::Whitelist(_) | Match::None => None,
        }
    }
}

impl Default for IgnoreConfig {
    fn default() -> Self {
        Self {
            paths: Gitignore::empty(),
            tokens: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Config {
    pub root_name: Option<RootName>,
    /// Absolute directories, `~` expanded, relative paths resolved against the config file.
    pub external_roots: BTreeMap<RootName, Utf8PathBuf>,
    pub scan: ScanConfig,
    pub containers: ContainerRules,
    pub check: CheckConfig,
    pub ignore: IgnoreConfig,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("could not read {path}: {source}")]
    Read {
        path: Utf8PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{path} is not valid TOML:\n{source}")]
    Parse {
        path: Utf8PathBuf,
        #[source]
        source: Box<toml::de::Error>,
    },
    #[error("{path}: `{field}`: {reason}")]
    Invalid {
        path: Utf8PathBuf,
        field: String,
        reason: String,
        /// Byte range in the file, for caret rendering.
        span: Option<Range<usize>>,
    },
    #[error("path {path:?} is not valid UTF-8")]
    NonUtf8Path { path: std::path::PathBuf },
}

/// Where a check runs from: the root directory, and the config that governs it.
#[derive(Debug, Clone)]
pub struct Discovered {
    pub root_dir: Utf8PathBuf,
    pub config_path: Option<Utf8PathBuf>,
    pub config: Config,
}

/// Walks up from `start` to the first directory holding @ref[anchr.toml]; failing that, the first
/// holding `.git`; failing that, `start` itself with default config.
pub fn discover(start: &Utf8Path) -> Result<Discovered, ConfigError> {
    let config_dir = start
        .ancestors()
        .find(|dir| dir.join(CONFIG_FILE_NAME).is_file());
    let root_dir = config_dir
        .or_else(|| start.ancestors().find(|dir| dir.join(".git").exists()))
        .unwrap_or(start)
        .to_path_buf();
    let (config, config_path) = load_for_root(&root_dir)?;
    Ok(Discovered {
        root_dir,
        config_path,
        config,
    })
}

// @noref[root_dir/anchr.toml]
/// Reads `root_dir/anchr.toml` if present; otherwise the defaults.
pub fn load_for_root(root_dir: &Utf8Path) -> Result<(Config, Option<Utf8PathBuf>), ConfigError> {
    let path = root_dir.join(CONFIG_FILE_NAME);
    if !path.is_file() {
        return Ok((Config::default(), None));
    }
    let text = std::fs::read_to_string(&path).map_err(|source| ConfigError::Read {
        path: path.clone(),
        source,
    })?;
    let config = Config::from_toml(&text, &path)?;
    Ok((config, Some(path)))
}

impl Config {
    /// `config_path` is where `text` came from; relative root paths resolve against its parent.
    pub fn from_toml(text: &str, config_path: &Utf8Path) -> Result<Self, ConfigError> {
        let raw: RawConfig = toml::from_str(text).map_err(|source| ConfigError::Parse {
            path: config_path.to_path_buf(),
            source: Box::new(source),
        })?;
        let base_dir = config_path.parent().unwrap_or(Utf8Path::new(""));
        Validator {
            path: config_path,
            base_dir,
        }
        .validate(raw)
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case", default)]
struct RawConfig {
    root: RawRootSection,
    roots: BTreeMap<String, Spanned<String>>,
    scan: RawScanSection,
    containers: RawContainersSection,
    check: RawCheckSection,
    ignore: RawIgnoreSection,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case", default)]
struct RawRootSection {
    name: Option<Spanned<String>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case", default)]
struct RawScanSection {
    include: Option<Vec<Spanned<String>>>,
    max_file_bytes: Option<Spanned<u64>>,
    parse_budget_ms: Option<Spanned<u64>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case", default)]
struct RawContainersSection {
    markdown: Option<Vec<Spanned<String>>>,
    plaintext: Option<Vec<Spanned<String>>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case", default)]
struct RawCheckSection {
    unverified: UnverifiedPolicy,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case", default)]
struct RawIgnoreSection {
    paths: Vec<Spanned<String>>,
    tokens: Vec<Spanned<String>>,
}

struct Validator<'a> {
    path: &'a Utf8Path,
    base_dir: &'a Utf8Path,
}

impl Validator<'_> {
    fn validate(&self, raw: RawConfig) -> Result<Config, ConfigError> {
        let root_name = raw
            .root
            .name
            .map(|name| self.root_name("root.name", &name))
            .transpose()?;

        let mut external_roots = BTreeMap::new();
        for (name, dir) in raw.roots {
            let field = format!("roots.{name}");
            let name = RootName::parse(&name)
                .map_err(|reason| self.invalid(&field, reason.to_string(), Some(dir.span())))?;
            external_roots.insert(name, self.root_dir(&field, &dir)?);
        }

        let include_patterns = raw
            .scan
            .include
            .map(|patterns| self.strings("scan.include", &patterns))
            .transpose()?;
        let include = include_patterns
            .as_deref()
            .map(|patterns| self.glob_set("scan.include", patterns))
            .transpose()?;

        let max_file_bytes = match raw.scan.max_file_bytes {
            None => DEFAULT_MAX_FILE_BYTES,
            Some(value) => {
                let bytes = *value.get_ref();
                if bytes == 0 || bytes > u64::from(u32::MAX) {
                    return Err(self.invalid(
                        "scan.max-file-bytes",
                        format!("must be between 1 and {}", u32::MAX),
                        Some(value.span()),
                    ));
                }
                bytes
            }
        };
        let parse_budget = match raw.scan.parse_budget_ms {
            None => DEFAULT_PARSE_BUDGET,
            Some(value) if *value.get_ref() == 0 => {
                return Err(self.invalid(
                    "scan.parse-budget-ms",
                    "must be at least 1".to_owned(),
                    Some(value.span()),
                ));
            }
            Some(value) => Duration::from_millis(*value.get_ref()),
        };

        let defaults = ContainerRules::default();
        let containers = ContainerRules {
            markdown_extensions: match raw.containers.markdown {
                Some(list) => self.extensions("containers.markdown", &list)?,
                None => defaults.markdown_extensions,
            },
            plaintext_extensions: match raw.containers.plaintext {
                Some(list) => self.extensions("containers.plaintext", &list)?,
                None => defaults.plaintext_extensions,
            },
        };

        let ignore = IgnoreConfig {
            paths: self.gitignore("ignore.paths", &raw.ignore.paths)?,
            tokens: self.noref_entries("ignore.tokens", &raw.ignore.tokens)?,
        };

        Ok(Config {
            root_name,
            external_roots,
            scan: ScanConfig {
                include,
                include_patterns: include_patterns.unwrap_or_default(),
                max_file_bytes,
                parse_budget,
            },
            containers,
            check: CheckConfig {
                unverified: raw.check.unverified,
            },
            ignore,
        })
    }

    fn root_name(&self, field: &str, value: &Spanned<String>) -> Result<RootName, ConfigError> {
        RootName::parse(value.get_ref()).map_err(|reason: RootNameError| {
            self.invalid(field, reason.to_string(), Some(value.span()))
        })
    }

    fn root_dir(&self, field: &str, value: &Spanned<String>) -> Result<Utf8PathBuf, ConfigError> {
        let raw = value.get_ref();
        if raw.is_empty() {
            return Err(self.invalid(field, "directory is empty".to_owned(), Some(value.span())));
        }
        let expanded = expand_home(raw).ok_or_else(|| {
            self.invalid(
                field,
                "`~` cannot be expanded: no home directory is set".to_owned(),
                Some(value.span()),
            )
        })?;
        let absolute = if expanded.is_absolute() {
            expanded
        } else {
            self.base_dir.join(expanded)
        };
        Ok(absolute)
    }

    fn strings(&self, field: &str, values: &[Spanned<String>]) -> Result<Vec<String>, ConfigError> {
        values
            .iter()
            .map(|value| {
                if value.get_ref().is_empty() {
                    Err(self.invalid(field, "empty pattern".to_owned(), Some(value.span())))
                } else {
                    Ok(value.get_ref().clone())
                }
            })
            .collect()
    }

    fn noref_entries(
        &self,
        field: &str,
        values: &[Spanned<String>],
    ) -> Result<Vec<NoRefEntry>, ConfigError> {
        let mut entries: Vec<NoRefEntry> = Vec::with_capacity(values.len());
        for value in values {
            let entry = NoRefEntry::parse(value.get_ref())
                .map_err(|reason| self.invalid(field, reason.to_string(), Some(value.span())))?;
            if entries.contains(&entry) {
                return Err(self.invalid(
                    field,
                    format!("duplicate entry `{entry}`"),
                    Some(value.span()),
                ));
            }
            entries.push(entry);
        }
        Ok(entries)
    }

    /// `[ignore] paths`: gitignore lines rooted at the config's directory, so `/x` anchors and
    /// `!x` re-includes exactly as they would in a `.gitignore` there.
    fn gitignore(&self, field: &str, values: &[Spanned<String>]) -> Result<Gitignore, ConfigError> {
        let mut builder = GitignoreBuilder::new(self.base_dir);
        for value in values {
            let line = value.get_ref();
            if line.trim().is_empty() || line.starts_with('#') {
                return Err(self.invalid(field, "empty pattern".to_owned(), Some(value.span())));
            }
            builder.add_line(None, line).map_err(|error| {
                self.invalid(field, format!("`{line}`: {error}"), Some(value.span()))
            })?;
        }
        builder
            .build()
            .map_err(|error| self.invalid(field, error.to_string(), None))
    }

    fn glob_set(&self, field: &str, patterns: &[String]) -> Result<GlobSet, ConfigError> {
        let mut builder = GlobSetBuilder::new();
        for pattern in patterns {
            let glob = GlobBuilder::new(pattern)
                .literal_separator(true)
                .build()
                .map_err(|error| {
                    self.invalid(field, format!("`{pattern}`: {}", error.kind()), None)
                })?;
            builder.add(glob);
        }
        builder
            .build()
            .map_err(|error| self.invalid(field, error.kind().to_string(), None))
    }

    fn extensions(
        &self,
        field: &str,
        values: &[Spanned<String>],
    ) -> Result<Vec<String>, ConfigError> {
        values
            .iter()
            .map(|value| {
                let raw = value.get_ref();
                let valid = !raw.is_empty()
                    && raw
                        .chars()
                        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-');
                if valid {
                    Ok(raw.to_ascii_lowercase())
                } else {
                    Err(self.invalid(
                        field,
                        format!("`{raw}` is not an extension; write it without the dot, e.g. `md`"),
                        Some(value.span()),
                    ))
                }
            })
            .collect()
    }

    fn invalid(&self, field: &str, reason: String, span: Option<Range<usize>>) -> ConfigError {
        ConfigError::Invalid {
            path: self.path.to_path_buf(),
            field: field.to_owned(),
            reason,
            span,
        }
    }
}

/// Expands a leading `~` or `~/`. `None` only when expansion is needed and no home is known.
fn expand_home(raw: &str) -> Option<Utf8PathBuf> {
    let Some(rest) = raw.strip_prefix('~') else {
        return Some(Utf8PathBuf::from(raw));
    };
    if !(rest.is_empty() || rest.starts_with('/')) {
        return Some(Utf8PathBuf::from(raw));
    }
    let home = home_dir()?;
    Some(home.join(rest.trim_start_matches('/')))
}

fn home_dir() -> Option<Utf8PathBuf> {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(var)
        .filter(|value| !value.is_empty())
        .and_then(|value| Utf8PathBuf::from_path_buf(value.into()).ok())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod config_tests;
