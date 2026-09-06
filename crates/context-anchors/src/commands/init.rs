//! The one command in milestone 1 that writes files. Rules: never overwrite without
//! `--force`, print every path touched, and be a no-op the second time.

use std::fs;
use std::io::Write;

use anchr_core::config::CONFIG_FILE_NAME;
use anyhow::{Context, bail};
use camino::{Utf8Path, Utf8PathBuf};
use serde_json::{Value, json};

use super::Outcome;
use crate::cli::{Agent, InitArgs};

const CONFIG_TEMPLATE: &str = include_str!("../../templates/anchr.toml");
const GUIDE_TEMPLATE: &str = include_str!("../../templates/ANCHR.md");
pub const GUIDE_FILE_NAME: &str = "ANCHR.md";
const CLAUDE_SETTINGS_PATH: &str = ".claude/settings.json";
/// Exit 2 with diagnostics on stderr is how a Claude Code hook feeds output back to the agent.
const CLAUDE_HOOK_COMMAND: &str = "anchr check --color never 1>&2 || exit 2";
const CLAUDE_HOOK_MATCHER: &str = "Edit|Write|MultiEdit|NotebookEdit";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Create,
    Overwrite,
    Unchanged,
    /// Exists with different contents and `--force` was not given.
    Kept,
}

struct Planned {
    path: Utf8PathBuf,
    contents: String,
    action: Action,
}

pub fn run(args: &InitArgs) -> anyhow::Result<Outcome> {
    let cwd = Utf8PathBuf::from_path_buf(
        std::env::current_dir().context("reading the current directory")?,
    )
    .map_err(|path| anyhow::anyhow!("current directory {} is not valid UTF-8", path.display()))?;
    let root = match &args.root {
        Some(root) if root.is_absolute() => root.clone(),
        Some(root) => cwd.join(root),
        None => cwd,
    };
    if !root.is_dir() {
        bail!("{root} is not a directory");
    }

    let mut plan = vec![plan_file(
        &root.join(CONFIG_FILE_NAME),
        CONFIG_TEMPLATE,
        args.force,
    )?];
    if args.agent != Agent::None {
        plan.push(plan_file(
            &root.join(GUIDE_FILE_NAME),
            GUIDE_TEMPLATE,
            args.force,
        )?);
    }
    if args.agent == Agent::Claude {
        plan.push(plan_claude_settings(&root)?);
    }

    let mut out = anstream::stdout().lock();
    for planned in &plan {
        let verb = match (planned.action, args.dry_run) {
            (Action::Create, false) => "created",
            (Action::Create, true) => "would create",
            (Action::Overwrite, false) => "overwrote",
            (Action::Overwrite, true) => "would overwrite",
            (Action::Unchanged, _) => "unchanged",
            (Action::Kept, _) => "kept (differs; pass --force to overwrite)",
        };
        if !args.dry_run && matches!(planned.action, Action::Create | Action::Overwrite) {
            if let Some(parent) = planned.path.parent() {
                fs::create_dir_all(parent).with_context(|| format!("creating {parent}"))?;
            }
            fs::write(&planned.path, &planned.contents)
                .with_context(|| format!("writing {}", planned.path))?;
        }
        writeln!(
            out,
            "{verb}: {}",
            relative_for_display(&root, &planned.path)
        )?;
    }

    if args.agent != Agent::None {
        writeln!(
            out,
            "\nAdd to AGENTS.md or CLAUDE.md: \"Before editing docs or comments, read ANCHR.md; \
             after editing, run `anchr check`.\""
        )?;
    }
    out.flush()?;
    Ok(Outcome::Clean)
}

fn plan_file(path: &Utf8Path, contents: &str, force: bool) -> anyhow::Result<Planned> {
    let action = match fs::read_to_string(path) {
        Ok(existing) if existing == contents => Action::Unchanged,
        Ok(_) if force => Action::Overwrite,
        Ok(_) => Action::Kept,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Action::Create,
        Err(error) => return Err(error).with_context(|| format!("reading {path}")),
    };
    Ok(Planned {
        path: path.to_path_buf(),
        contents: contents.to_owned(),
        action,
    })
}

/// Merges the hook into `.claude/settings.json` by read-modify-write on a JSON value, so
/// every key the file already has is preserved. Additive, so `--force` is not required.
fn plan_claude_settings(root: &Utf8Path) -> anyhow::Result<Planned> {
    let path = root.join(CLAUDE_SETTINGS_PATH);
    let hook_entry = json!({
        "matcher": CLAUDE_HOOK_MATCHER,
        "hooks": [{ "type": "command", "command": CLAUDE_HOOK_COMMAND }],
    });

    let existing = match fs::read_to_string(&path) {
        Ok(text) => Some(text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error).with_context(|| format!("reading {path}")),
    };
    let mut settings: Value = match &existing {
        None => json!({}),
        Some(text) => serde_json::from_str(text).with_context(|| {
            format!(
                "{path} is not valid JSON, so the hook was not merged. Add this entry to \
                 `hooks.PostToolUse` yourself:\n{}",
                serde_json::to_string_pretty(&hook_entry).unwrap_or_default()
            )
        })?,
    };

    let Some(object) = settings.as_object_mut() else {
        bail!("{path} is valid JSON but not an object; the hook was not merged");
    };
    let hooks = object
        .entry("hooks")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("{path}: `hooks` is not an object"))?;
    let post_tool_use = hooks
        .entry("PostToolUse")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or_else(|| anyhow::anyhow!("{path}: `hooks.PostToolUse` is not an array"))?;

    let already_present = post_tool_use.iter().any(|entry| {
        entry["hooks"].as_array().is_some_and(|hooks| {
            hooks.iter().any(|hook| {
                hook["command"]
                    .as_str()
                    .is_some_and(|command| command.starts_with("anchr check"))
            })
        })
    });
    if !already_present {
        post_tool_use.push(hook_entry);
    }

    let mut contents = serde_json::to_string_pretty(&settings)?;
    contents.push('\n');
    let action = match existing {
        Some(text) if text == contents => Action::Unchanged,
        Some(_) => Action::Overwrite,
        None => Action::Create,
    };
    Ok(Planned {
        path,
        contents,
        action,
    })
}

fn relative_for_display<'a>(root: &Utf8Path, path: &'a Utf8Path) -> &'a Utf8Path {
    path.strip_prefix(root).unwrap_or(path)
}
