//! Roots: the namespaces references resolve in, and the paths of files inside them.

use std::collections::BTreeMap;
use std::fmt;

use camino::{Utf8Component, Utf8Path, Utf8PathBuf};

use crate::config::{self, Config, ConfigError};
use crate::marker::RelPath;

pub const MAX_ROOT_NAME_BYTES: usize = 64;

/// Name of a root: the namespace a reference resolves in. `[A-Za-z0-9_-]+`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RootName(String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, thiserror::Error)]
pub enum RootNameError {
    #[error("root name is empty")]
    Empty,
    #[error("root name is {len} bytes; the limit is {MAX_ROOT_NAME_BYTES}")]
    TooLong { len: usize },
    #[error("root name contains `{ch}`; only letters, digits, `_` and `-` are allowed")]
    InvalidChar { ch: char },
}

impl RootName {
    pub fn parse(raw: &str) -> Result<Self, RootNameError> {
        if raw.is_empty() {
            return Err(RootNameError::Empty);
        }
        if raw.len() > MAX_ROOT_NAME_BYTES {
            return Err(RootNameError::TooLong { len: raw.len() });
        }
        if let Some(ch) = raw.chars().find(|ch| !is_root_name_char(*ch)) {
            return Err(RootNameError::InvalidChar { ch });
        }
        Ok(Self(raw.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

pub(crate) fn is_root_name_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_' || ch == '-'
}

impl fmt::Display for RootName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Root-relative path of a scanned file. Unlike [`RelPath`], any UTF-8 name is allowed: a
/// file called `My Notes.md` is scanned even though no `@ref` can name it. Separators are
/// always `/`, so paths compare equal to reference targets and print the same on every platform.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FilePath(Utf8PathBuf);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FilePathError {
    #[error("file path is empty")]
    Empty,
    #[error("file path `{path}` is not relative to the root")]
    NotRelative { path: Utf8PathBuf },
}

impl FilePath {
    pub fn new(path: Utf8PathBuf) -> Result<Self, FilePathError> {
        let path = with_forward_slashes(path);
        if path.as_str().is_empty() {
            return Err(FilePathError::Empty);
        }
        let escapes = path.components().any(|component| {
            matches!(
                component,
                Utf8Component::RootDir | Utf8Component::Prefix(_) | Utf8Component::ParentDir
            )
        });
        if escapes {
            return Err(FilePathError::NotRelative { path });
        }
        Ok(Self(path))
    }

    pub fn as_path(&self) -> &Utf8Path {
        &self.0
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    pub fn extension(&self) -> Option<&str> {
        self.0.extension()
    }
}

fn with_forward_slashes(path: Utf8PathBuf) -> Utf8PathBuf {
    if cfg!(windows) && path.as_str().contains('\\') {
        Utf8PathBuf::from(path.as_str().replace('\\', "/"))
    } else {
        path
    }
}

impl From<&RelPath> for FilePath {
    fn from(path: &RelPath) -> Self {
        Self(path.as_path().to_path_buf())
    }
}

impl fmt::Display for FilePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0.as_str())
    }
}

/// A root that exists on disk, with the config that governs how it is scanned.
#[derive(Debug, Clone)]
pub struct Root {
    pub name: RootName,
    pub dir: Utf8PathBuf,
    pub config: Config,
}

#[derive(Debug, Clone)]
pub enum RootStatus {
    Present(Box<Root>),
    /// Declared in config but the directory is not on disk. References into it are
    /// unverified, never errors: "not visible from here" is not "gone".
    Absent {
        name: RootName,
        declared_dir: Utf8PathBuf,
    },
}

impl RootStatus {
    pub fn name(&self) -> &RootName {
        match self {
            RootStatus::Present(root) => &root.name,
            RootStatus::Absent { name, .. } => name,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RootSetError {
    #[error(
        "the directory name `{basename}` is not a valid root name ({reason}); set `[root] name` \
         in {config_file}"
    )]
    InvalidCurrentName {
        basename: String,
        reason: RootNameError,
        config_file: &'static str,
    },
    #[error("external root `{name}` collides with the current root's name")]
    Collision { name: RootName },
    #[error("loading config for external root `{name}` at {dir}: {source}")]
    ExternalConfig {
        name: RootName,
        dir: Utf8PathBuf,
        #[source]
        source: Box<ConfigError>,
    },
}

/// The current root plus every external root its config declares.
#[derive(Debug, Clone)]
pub struct RootSet {
    current: RootName,
    roots: BTreeMap<RootName, RootStatus>,
}

impl RootSet {
    /// `current_dir` must be absolute. External roots load their own `anchr.toml` for scan and
    /// container settings; their `[roots]` and `[check]` tables are not consulted.
    pub fn load(current_dir: Utf8PathBuf, current_config: Config) -> Result<Self, RootSetError> {
        let current = match &current_config.root_name {
            Some(name) => name.clone(),
            None => root_name_from_dir(&current_dir)?,
        };

        let mut roots = BTreeMap::new();
        for (name, declared_dir) in &current_config.external_roots {
            if *name == current {
                return Err(RootSetError::Collision { name: name.clone() });
            }
            let status = if declared_dir.is_dir() {
                let (config, _) = config::load_for_root(declared_dir).map_err(|source| {
                    RootSetError::ExternalConfig {
                        name: name.clone(),
                        dir: declared_dir.clone(),
                        source: Box::new(source),
                    }
                })?;
                RootStatus::Present(Box::new(Root {
                    name: name.clone(),
                    dir: declared_dir.clone(),
                    config,
                }))
            } else {
                RootStatus::Absent {
                    name: name.clone(),
                    declared_dir: declared_dir.clone(),
                }
            };
            roots.insert(name.clone(), status);
        }
        roots.insert(
            current.clone(),
            RootStatus::Present(Box::new(Root {
                name: current.clone(),
                dir: current_dir,
                config: current_config,
            })),
        );
        Ok(Self { current, roots })
    }

    pub fn current_name(&self) -> &RootName {
        &self.current
    }

    pub fn current(&self) -> &Root {
        match self.roots.get(&self.current) {
            Some(RootStatus::Present(root)) => root,
            // `load` always inserts the current root as Present.
            _ => unreachable!("current root is always present"),
        }
    }

    pub fn status(&self, name: &RootName) -> Option<&RootStatus> {
        self.roots.get(name)
    }

    pub fn present(&self) -> impl Iterator<Item = &Root> {
        self.roots.values().filter_map(|status| match status {
            RootStatus::Present(root) => Some(&**root),
            RootStatus::Absent { .. } => None,
        })
    }

    pub fn external(&self) -> impl Iterator<Item = &RootStatus> {
        self.roots
            .iter()
            .filter(|(name, _)| **name != self.current)
            .map(|(_, status)| status)
    }

    pub fn names(&self) -> impl Iterator<Item = &RootName> {
        self.roots.keys()
    }
}

fn root_name_from_dir(dir: &Utf8Path) -> Result<RootName, RootSetError> {
    let basename = dir.file_name().unwrap_or_default();
    RootName::parse(basename).map_err(|reason| RootSetError::InvalidCurrentName {
        basename: basename.to_owned(),
        reason,
        config_file: config::CONFIG_FILE_NAME,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn root_names_accept_the_documented_charset() {
        assert_eq!(
            RootName::parse("claude-Code_2").unwrap().as_str(),
            "claude-Code_2"
        );
    }

    #[test]
    fn root_names_reject_empty_dots_slashes_and_overlong() {
        assert_eq!(RootName::parse(""), Err(RootNameError::Empty));
        assert_eq!(
            RootName::parse("a.b"),
            Err(RootNameError::InvalidChar { ch: '.' })
        );
        assert_eq!(
            RootName::parse("a/b"),
            Err(RootNameError::InvalidChar { ch: '/' })
        );
        let long = "x".repeat(MAX_ROOT_NAME_BYTES + 1);
        assert_eq!(
            RootName::parse(&long),
            Err(RootNameError::TooLong { len: 65 })
        );
    }

    #[test]
    fn file_paths_allow_any_relative_utf8_name() {
        let path = FilePath::new(Utf8PathBuf::from("docs/My Notes #1.md")).unwrap();
        assert_eq!(path.extension(), Some("md"));
        assert!(matches!(
            FilePath::new(Utf8PathBuf::from("/abs")),
            Err(FilePathError::NotRelative { .. })
        ));
        assert!(matches!(
            FilePath::new(Utf8PathBuf::from("a/../b")),
            Err(FilePathError::NotRelative { .. })
        ));
        assert_eq!(
            FilePath::new(Utf8PathBuf::from("")),
            Err(FilePathError::Empty)
        );
    }

    #[cfg(windows)]
    #[test]
    fn file_paths_use_forward_slashes_on_windows() {
        let path = FilePath::new(Utf8PathBuf::from("src\\lib.rs")).unwrap();
        assert_eq!(path.as_str(), "src/lib.rs");
    }

    #[test]
    fn a_rel_path_converts_to_a_file_path() {
        let rel = RelPath::parse("src/lib.rs").unwrap();
        assert_eq!(FilePath::from(&rel).as_str(), "src/lib.rs");
    }

    #[test]
    fn root_set_names_the_current_root_after_its_directory() {
        let dir = tempfile::tempdir().unwrap();
        let project = Utf8PathBuf::from_path_buf(dir.path().join("my-project")).unwrap();
        std::fs::create_dir(&project).unwrap();
        let set = RootSet::load(project.clone(), Config::default()).unwrap();
        assert_eq!(set.current_name().as_str(), "my-project");
        assert_eq!(set.current().dir, project);
        assert_eq!(set.names().count(), 1);
    }

    #[test]
    fn an_unnameable_directory_needs_an_explicit_root_name() {
        let dir = tempfile::tempdir().unwrap();
        let project = Utf8PathBuf::from_path_buf(dir.path().join("my.project")).unwrap();
        std::fs::create_dir(&project).unwrap();
        assert!(matches!(
            RootSet::load(project.clone(), Config::default()),
            Err(RootSetError::InvalidCurrentName { .. })
        ));
        let config = Config {
            root_name: Some(RootName::parse("explicit").unwrap()),
            ..Config::default()
        };
        assert_eq!(
            RootSet::load(project, config)
                .unwrap()
                .current_name()
                .as_str(),
            "explicit"
        );
    }

    #[test]
    fn external_roots_are_present_or_absent_and_may_not_collide() {
        let dir = tempfile::tempdir().unwrap();
        let base = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
        let project = base.join("project");
        let plugin = base.join("plugin");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(&plugin).unwrap();

        let mut config = Config::default();
        config
            .external_roots
            .insert(RootName::parse("plugin").unwrap(), plugin.clone());
        config
            .external_roots
            .insert(RootName::parse("missing").unwrap(), base.join("nope"));
        let set = RootSet::load(project.clone(), config).unwrap();

        assert!(matches!(
            set.status(&RootName::parse("plugin").unwrap()),
            Some(RootStatus::Present(root)) if root.dir == plugin
        ));
        assert!(matches!(
            set.status(&RootName::parse("missing").unwrap()),
            Some(RootStatus::Absent { .. })
        ));
        assert_eq!(set.present().count(), 2);
        assert_eq!(set.external().count(), 2);

        let mut colliding = Config::default();
        colliding
            .external_roots
            .insert(RootName::parse("project").unwrap(), plugin);
        assert!(matches!(
            RootSet::load(project, colliding),
            Err(RootSetError::Collision { .. })
        ));
    }
}
