//! `anchr.toml`: parsed defensively, then validated once into [`Config`].

use std::collections::BTreeMap;
use std::ops::Range;
use std::time::Duration;

use camino::{Utf8Path, Utf8PathBuf};
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
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
    pub exclude_patterns: Vec<String>,
    pub max_file_bytes: u64,
    pub parse_budget: Duration,
}

impl Default for ScanConfig {
    fn default() -> Self {
        Self {
            include: None,
            include_patterns: Vec::new(),
            exclude_patterns: Vec::new(),
            max_file_bytes: DEFAULT_MAX_FILE_BYTES,
            parse_budget: DEFAULT_PARSE_BUDGET,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CheckConfig {
    pub unverified: UnverifiedPolicy,
}

/// What `coverage` leaves alone. Nothing here affects `check`.
#[derive(Debug, Clone)]
pub struct CoverageConfig {
    /// Files still scanned and checked, but never asked for candidates.
    pub exclude: GlobSet,
    pub exclude_patterns: Vec<String>,
    /// Strings that are never references anywhere in the root.
    pub ignore: Vec<NoRefEntry>,
}

impl Default for CoverageConfig {
    fn default() -> Self {
        Self {
            exclude: GlobSet::empty(),
            exclude_patterns: Vec::new(),
            ignore: Vec::new(),
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
    pub coverage: CoverageConfig,
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

/// Walks up from `start` to the first directory holding `anchr.toml`; failing that, the first
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
    coverage: RawCoverageSection,
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
    exclude: Vec<Spanned<String>>,
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
struct RawCoverageSection {
    exclude: Vec<Spanned<String>>,
    ignore: Vec<Spanned<String>>,
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
        let exclude_patterns = self.strings("scan.exclude", &raw.scan.exclude)?;
        self.glob_set("scan.exclude", &exclude_patterns)?;

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

        let coverage_exclude_patterns = self.strings("coverage.exclude", &raw.coverage.exclude)?;
        let coverage = CoverageConfig {
            exclude: self.glob_set("coverage.exclude", &coverage_exclude_patterns)?,
            exclude_patterns: coverage_exclude_patterns,
            ignore: self.noref_entries("coverage.ignore", &raw.coverage.ignore)?,
        };

        Ok(Config {
            root_name,
            external_roots,
            scan: ScanConfig {
                include,
                include_patterns: include_patterns.unwrap_or_default(),
                exclude_patterns,
                max_file_bytes,
                parse_budget,
            },
            containers,
            check: CheckConfig {
                unverified: raw.check.unverified,
            },
            coverage,
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
mod tests {
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
        assert!(config.coverage.exclude_patterns.is_empty());
        assert!(config.coverage.ignore.is_empty());
        assert!(!config.coverage.exclude.is_match("anything.md"));
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
            exclude = ["vendor/**"]
            max-file-bytes = 4096
            parse-budget-ms = 250

            [containers]
            markdown = ["MD", "mdx"]
            plaintext = ["txt", "text"]

            [check]
            unverified = "error"

            [coverage]
            exclude = ["docs/research/**"]
            ignore = ["CLAUDE.md", "src/legacy/"]
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
        assert_eq!(config.scan.exclude_patterns, vec!["vendor/**"]);
        assert_eq!(config.scan.max_file_bytes, 4096);
        assert_eq!(config.scan.parse_budget, Duration::from_millis(250));
        assert_eq!(config.containers.markdown_extensions, vec!["md", "mdx"]);
        assert_eq!(config.containers.plaintext_extensions, vec!["txt", "text"]);
        assert_eq!(config.check.unverified, UnverifiedPolicy::Error);
        assert_eq!(config.coverage.exclude_patterns, vec!["docs/research/**"]);
        assert!(config.coverage.exclude.is_match("docs/research/a/b.md"));
        assert!(!config.coverage.exclude.is_match("docs/design/a.md"));
        assert_eq!(
            config
                .coverage
                .ignore
                .iter()
                .map(NoRefEntry::as_str)
                .collect::<Vec<_>>(),
            vec!["CLAUDE.md", "src/legacy/"]
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
            ("[scan]\nexclude = [\"\"]\n", "scan.exclude"),
            (
                "[containers]\nmarkdown = [\".md\"]\n",
                "containers.markdown",
            ),
            ("[coverage]\nexclude = [\"\"]\n", "coverage.exclude"),
            ("[coverage]\nignore = [\"\"]\n", "coverage.ignore"),
            ("[coverage]\nignore = [\"a b\"]\n", "coverage.ignore"),
            ("[coverage]\nignore = [\"a,b\"]\n", "coverage.ignore"),
            ("[coverage]\nignore = [\"x\", \"x\"]\n", "coverage.ignore"),
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
        let error = parse("[coverage]\nexclude = [\"docs/[\"]\n").unwrap_err();
        assert!(matches!(error, ConfigError::Invalid { field, .. } if field == "coverage.exclude"));
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
}
