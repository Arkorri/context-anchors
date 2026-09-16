//! The human report followed by one GitHub Actions workflow command per location, so a finding
//! lands on the line of the pull-request diff. Annotations are a presentation of the same report,
//! never a third contract: the title is the JSON code, the message is the kind's text. Every
//! location is emitted; the log is the record, whatever the web UI truncates.

use std::collections::BTreeMap;
use std::io::Write;

use anchr_core::diagnostic::{Diagnostic, Locations, Report, Severity};
use anchr_core::root::{FilePath, RootName};
use annotate_snippets::Renderer;
use camino::{Utf8Path, Utf8PathBuf};

use super::{human, json};

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod github_tests;

/// `workspace` is the directory GitHub reads `file=` against: the checkout, which may lie above
/// the current directory when a step sets `working-directory`.
pub fn write(out: &mut impl Write, report: &Report, workspace: &Utf8Path) -> std::io::Result<()> {
    write_with(out, &Renderer::styled(), report, workspace)
}

fn write_with(
    out: &mut impl Write,
    renderer: &Renderer,
    report: &Report,
    workspace: &Utf8Path,
) -> std::io::Result<()> {
    human::write_with(out, renderer, report)?;
    write_annotations(out, report, workspace)
}

/// One `::error` or `::warning` line per location, in report order. A location whose root lies
/// outside the workspace cannot be attached to a file, so it is spelled out in the message.
///
/// Both sides of the containment test are canonicalised first: the workspace comes from an
/// environment variable and the root from the process's working directory, and the two may
/// spell one directory differently (a symlinked temp dir on macOS, a short name on Windows).
fn write_annotations(
    out: &mut impl Write,
    report: &Report,
    workspace: &Utf8Path,
) -> std::io::Result<()> {
    let workspace = canonical(workspace);
    let root_dirs: BTreeMap<&RootName, Utf8PathBuf> = report
        .root_dirs
        .iter()
        .map(|(root, dir)| (root, canonical(dir)))
        .collect();
    for diagnostic in &report.diagnostics {
        let level = level(diagnostic.severity);
        let title = format!("title={}", escape_property(json::code(&diagnostic.kind)));
        let message = message(diagnostic);
        match &diagnostic.locations {
            Locations::Sites(sites) => {
                for site in sites {
                    let (line, col) = (site.line_col.line, site.line_col.col);
                    let file = root_dirs
                        .get(&site.site.root)
                        .and_then(|dir| workspace_relative(dir, &workspace, &site.site.path));
                    let line = match file {
                        Some(file) => command(
                            level,
                            &[
                                format!("file={}", escape_property(&file)),
                                format!("line={line}"),
                                format!("col={col}"),
                                title.clone(),
                            ],
                            &message,
                        ),
                        None => {
                            let at =
                                format!("{}:{}:{line}:{col}: ", site.site.root, site.site.path);
                            command(
                                level,
                                std::slice::from_ref(&title),
                                &format!("{at}{message}"),
                            )
                        }
                    };
                    writeln!(out, "{line}")?;
                }
            }
            Locations::Files(files) => {
                for file in files {
                    let path = root_dirs
                        .get(&file.root)
                        .and_then(|dir| workspace_relative(dir, &workspace, &file.path));
                    let line = match path {
                        Some(path) => command(
                            level,
                            &[format!("file={}", escape_property(&path)), title.clone()],
                            &message,
                        ),
                        None => {
                            let at = format!("{}:{}: ", file.root, file.path);
                            command(
                                level,
                                std::slice::from_ref(&title),
                                &format!("{at}{message}"),
                            )
                        }
                    };
                    writeln!(out, "{line}")?;
                }
            }
            Locations::Roots(roots) => {
                for root in roots {
                    let message = format!("{message} (root `{root}`)");
                    writeln!(
                        out,
                        "{}",
                        command(level, std::slice::from_ref(&title), &message)
                    )?;
                }
            }
        }
    }
    Ok(())
}

fn level(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Unverified => "warning",
    }
}

/// The kind's text, then the suggestion and the hint the human report would show as help lines.
fn message(diagnostic: &Diagnostic) -> String {
    let mut message = diagnostic.kind.to_string();
    if let Some(suggestion) = &diagnostic.suggestion {
        message.push_str(&format!("; did you mean `{suggestion}`?"));
    }
    if let Some(hint) = diagnostic.kind.hint() {
        message.push_str(&format!("; {hint}"));
    }
    message
}

/// The canonical spelling of a directory when it exists, and the given one otherwise, so a
/// report built from an in-memory world still renders.
fn canonical(dir: &Utf8Path) -> Utf8PathBuf {
    dunce::canonicalize(dir)
        .ok()
        .and_then(|path| Utf8PathBuf::from_path_buf(path).ok())
        .unwrap_or_else(|| dir.to_path_buf())
}

/// The path GitHub needs: `path` under `root_dir`, spelled relative to `workspace` with `/`
/// separators. `None` when the root is not inside the workspace.
fn workspace_relative(
    root_dir: &Utf8Path,
    workspace: &Utf8Path,
    path: &FilePath,
) -> Option<String> {
    let relative = root_dir.strip_prefix(workspace).ok()?;
    let mut segments: Vec<&str> = relative.components().map(|c| c.as_str()).collect();
    segments.push(path.as_str());
    Some(segments.join("/"))
}

/// `::level key=value,key=value::message`, or `::level::message` with no properties. The
/// properties arrive already escaped.
fn command(level: &str, properties: &[String], message: &str) -> String {
    let message = escape_data(message);
    if properties.is_empty() {
        format!("::{level}::{message}")
    } else {
        format!("::{level} {}::{message}", properties.join(","))
    }
}

/// What the runner's `escapeData` does. `%` first, or an escaped newline would be re-escaped.
fn escape_data(text: &str) -> String {
    text.replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}

/// What the runner's `escapeProperty` does: the data escapes plus the two property delimiters.
fn escape_property(text: &str) -> String {
    escape_data(text).replace(':', "%3A").replace(',', "%2C")
}
