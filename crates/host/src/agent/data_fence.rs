//! Clauseo's own data folder is off-limits to agents (decision record 0030, "The data folder
//! fence"). Under the jail (tiers A and B) stage C hides `<data>` outside the session's own
//! folders; with no sandbox (tier C) two lines stand in for it:
//!
//! - Claude's deny rules ([`claude_deny_rules`]): `Read(//…)` and `Edit(//…)` for each
//!   Clauseo-only subtree and file of `<data>` ([`DENIED`]) and, enumerated at each launch,
//!   every entry of `<data>` that is not the session's own (`launch.rs`: other matters' notes,
//!   other run folders, `run.old-*`, anything else), in the flag settings of every Claude
//!   session. Claude Code applies them to its file tools and to the file commands it recognises
//!   in Bash. A deny rule has no exceptions, so the session's own run folder, this matter's notes
//!   and Claude's plans folder are left out by naming their siblings instead, and so are the
//!   tool results Claude saves in its own project folder. Run folders, matters' notes and other
//!   matters' Claude projects made after the launch are the accepted residual of these rules.
//! - The host's auto-deny ([`FenceCheck`]): a permission request whose paths ([`candidates`]:
//!   the request's locations and diffs, `rawInput`'s path keys and every string in it shaped
//!   like a path, the path words of a shell command and the targets of its `ln`, and any path
//!   that starts, inside a command's word, where the data folder's text does) resolve into
//!   `<data>` and outside the session's own folders, or whose search roots
//!   ([`candidates_roots`]: a Glob's or Grep's folder, where its pattern may start, the folders
//!   a recursive command walks) hold `<data>` or lie in it so, is answered with its reject-once
//!   option, never shown to the lawyer, when Claude (or Codex) asks.
//!   Resolving takes `..`, `~` and `~user` and follows links, a dangling one too; a link loop
//!   counts as inside. The own folders are the session's run folder, this matter's notes and
//!   Claude's plans folder, each only while it resolves to exactly its place under `<data>`'s
//!   canonical path (one swapped for a link counts for nothing), and the matter folder unless it
//!   holds `<data>` or is a link into it. It runs on the matter thread, since it stats the file
//!   system. ACP's answer to a permission request carries no reason, and the adapters tell the
//!   model only that the user refused; for the subtrees Claude's rules name, Claude Code's own
//!   denial comes first and says the path is denied.
//!
//! What neither catches: a script the agent writes that opens a file it names in parts or
//! builds at run time, a command that reaches a file through a variable or a `cd` first, a
//! link inside a searched folder, and anything an agent runs without asking (decision record
//! 0030 lists them). Only the jail or an OS sandbox stops those.

use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

use clauseo_matters::{HeldOpen, held_open_at};
use clauseo_protocol::MatterPath;
pub(crate) use input::{candidates, candidates_roots, candidates_words};
pub(crate) use launch::{LaunchDeny, claude_projects, launch_deny};

/// What of `<data>` only Clauseo uses, named whether or not it exists at the launch (the
/// launch adds every other entry that exists then, `launch.rs`), as rule patterns under it,
/// each denied to Claude for Read and Edit: the skill packs' sources, the version store, the
/// logs, the updater's downloads,
/// the usage counts, the Windows sandbox's ledger, the limits reader's state, the browser's config,
/// the settings file, the updater's marker, the counts' first-act marker, the host's store, the
/// search index (every matter's prompts and answers). Not the built-in plugin ([`SHARED`]).
/// Under `agents`: Codex's home, the installs, the runtime and npm folders, the sign-in shim and
/// markers, and in Claude's own home its login, its settings, its transcripts (every project's,
/// with its subagents' and its memory; not the whole of `projects`, which also holds the large
/// tool results the model must Read back, `launch.rs`), its skills and
/// plugins, and what it keeps across sessions, so across matters: the prompt history, the
/// edited files' copies, the to-do lists and the shell snapshots. Not Claude's home whole:
/// Claude Code writes its plan note to `plans` there with its own Edit tool, and a deny rule
/// has no exceptions.
pub(crate) const DENIED: [&str; 34] = [
    "skill-packs/**",
    "matters/**",
    "logs/**",
    "updates/**",
    "counts/**",
    "sandbox/**",
    "limits/**",
    "browser/**",
    clauseo_settings::SETTINGS_FILE,
    "update.json",
    clauseo_counts::FIRST_ACT,
    "state.sqlite*",
    "search.sqlite*",
    "agents/codex-acp/**",
    "agents/claude-acp/install/**",
    "agents/claude-acp/login-source",
    "agents/claude-acp/no-reuse",
    "agents/claude-acp/home/.credentials.json",
    "agents/claude-acp/home/.claude.json*",
    "agents/claude-acp/home/settings.json",
    "agents/claude-acp/home/projects/*/*.jsonl",
    "agents/claude-acp/home/projects/*/*/subagents/**",
    "agents/claude-acp/home/projects/*/memory/**",
    "agents/claude-acp/home/skills/**",
    "agents/claude-acp/home/plugins/**",
    "agents/claude-acp/home/history.jsonl",
    "agents/claude-acp/home/file-history/**",
    "agents/claude-acp/home/todos/**",
    "agents/claude-acp/home/shell-snapshots/**",
    "agents/runtime/**",
    "agents/downloads/**",
    "agents/npm-cache/**",
    "agents/signin/**",
    "agents/npmrc*",
];

/// What of `<data>` every session may read, with the reason: neither denied to Claude nor listed
/// per launch, and an own folder of every session for the auto-deny (while it resolves to its
/// place under `<data>`'s canonical path).
pub(crate) const SHARED: [(&str, &str); 1] = [(
    "profile",
    "holds only the built-in plugin, the same for every matter; the model must Read files in \
     its skills (legal-drafting's section-checklists.md)",
)];

/// Claude's `permissions.deny` rules for a launch: for the data folder spelled each way in
/// `deny.spellings` (its canonical path first, then the path as configured when that differs),
/// `Read` and `Edit` for each of `deny.targets`. A spelling that is not UTF-8 gets none (no
/// rule could name it).
pub(crate) fn claude_deny_rules(deny: &LaunchDeny) -> Vec<String> {
    let mut rules = Vec::new();
    for spelling in &deny.spellings {
        let Some(base) = spelling.to_str().map(|text| rule_path(text, cfg!(windows))) else {
            continue;
        };
        for target in &deny.targets {
            for tool in ["Read", "Edit"] {
                let rule = format!("{tool}({base}/{target})");
                if !rules.contains(&rule) {
                    rules.push(rule);
                }
            }
        }
    }
    rules
}

/// The built-in plugin's skills folder under `<data>` (`clauseo_profile`'s `claude_skills`):
/// Clauseo's own skill assets, the same for every matter, which its skills send the model to
/// Read (legal-drafting's `section-checklists.md`).
pub(crate) const BUILT_IN_SKILLS: [&str; 3] = ["profile", "claude-plugin", "skills"];

/// Claude's `permissions.allow` rules for a launch: `Read` of the built-in skills' folder
/// ([`BUILT_IN_SKILLS`]) and all under it, for each spelling of `<data>`, so a built-in skill
/// never asks the lawyer. Read only, and nothing wider. A spelling that is not UTF-8 gets none.
pub(crate) fn claude_allow_rules(deny: &LaunchDeny) -> Vec<String> {
    let skills: Vec<String> = BUILT_IN_SKILLS.iter().map(|name| escape(name)).collect();
    let skills = skills.join("/");
    let mut rules = Vec::new();
    for spelling in &deny.spellings {
        let Some(base) = spelling.to_str().map(|text| rule_path(text, cfg!(windows))) else {
            continue;
        };
        let rule = format!("Read({base}/{skills}/**)");
        if !rules.contains(&rule) {
            rules.push(rule);
        }
    }
    rules
}

/// An absolute path in Claude Code's rule form, `//<path from the root>`, glob characters
/// escaped. On Windows (`windows`) separators become `/` and a drive `C:` becomes `/c`, as
/// Claude Code normalises a Windows path before it matches (`C:\Users\a` is `/c/Users/a`), a
/// path to this machine's own share (`\\localhost\c$\x`, `\\?\C:\x`) put in its drive form first
/// ([`resolve::drive_form`]).
pub(crate) fn rule_path(text: &str, windows: bool) -> String {
    let mut path = if windows {
        resolve::drive_form(text)
            .unwrap_or_else(|| text.to_owned())
            .replace('\\', "/")
    } else {
        text.to_owned()
    };
    if windows {
        let mut chars = path.chars();
        if let (Some(drive), Some(':')) = (chars.next(), chars.next())
            && drive.is_ascii_alphabetic()
        {
            path = format!("/{}{}", drive.to_ascii_lowercase(), chars.as_str());
        }
    }
    let path = escape(&path);
    format!("//{}", path.trim_start_matches('/'))
}

/// `text` with the gitignore glob characters (`*`, `?`, `[`, `]`, `\`) escaped by `\`.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '*' | '?' | '[' | ']' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Whether `plans` is a plans folder in one of Clauseo's agents' homes under `data`
/// (`<data>/agents/<agent id>/home/plans`, `Locations::agent_home`), where Claude Code writes
/// its plan note (`agent/plan_note.rs`) when its home is Clauseo's.
pub(crate) fn is_agent_plans(data: &Path, plans: &Path) -> bool {
    let Ok(place) = plans.strip_prefix(data) else {
        return false;
    };
    let names: Vec<_> = place.components().collect();
    matches!(
        names.as_slice(),
        [Component::Normal(agents), Component::Normal(_), Component::Normal(home), Component::Normal(folder)]
            if *agents == "agents" && *home == "home" && *folder == "plans"
    )
}

/// A permission request's paths to check against the data folder, on the matter thread.
#[derive(Debug, Clone)]
pub(crate) struct FenceCheck {
    /// `<data>` as configured; resolved by the check.
    pub(crate) data: PathBuf,
    /// The session's own folders in `<data>`, as `<data>` (as configured) joined with their
    /// place in Clauseo's layout: its run folder, this matter's notes and Claude's plans folder.
    /// Each counts only while it resolves to exactly `<data>`'s canonical path joined with that
    /// place: one that is, or sits under, a link counts for nothing.
    pub(crate) allowed: Vec<PathBuf>,
    /// The agent's working folder (the matter folder): its own too, and the folder a relative
    /// path is taken from. It counts for nothing when it resolves to `<data>` or a folder above
    /// it, or when it is a link into `<data>`.
    pub(crate) cwd: PathBuf,
    /// The lawyer's home, for `~`; none when it is not known.
    pub(crate) home: Option<PathBuf>,
    /// The request's [`candidates`].
    pub(crate) paths: Vec<PathBuf>,
    /// The folders the request may search the whole of ([`candidates_roots`]): one that holds
    /// `<data>` touches it too.
    pub(crate) roots: Vec<PathBuf>,
    /// Every word of the request's shell command ([`candidates_words`]), searched for the data
    /// folder's text.
    pub(crate) words: Vec<String>,
}

impl FenceCheck {
    /// Whether any path resolves into `<data>` and outside every own folder, comparing ignoring
    /// case on Windows and macOS, whose file systems do. Stats the file system.
    pub(crate) fn touches_data(&self) -> bool {
        self.touches_data_as(cfg!(any(windows, target_os = "macos")))
    }

    /// [`Self::touches_data`], ignoring case when `insensitive`. A path that passes through a
    /// link loop counts as in `<data>`, as does every path when `<data>` itself does not
    /// resolve.
    pub(crate) fn touches_data_as(&self, insensitive: bool) -> bool {
        let home = self.home.as_deref();
        let Some(real_data) = resolve::resolve(&self.data, &self.cwd, home) else {
            return !self.paths.is_empty() || !self.words.is_empty() || !self.roots.is_empty();
        };
        let data = key(&real_data, insensitive);
        let own = self.own(&real_data, insensitive);
        let inside = |found: &Option<PathBuf>| match found {
            None => true,
            Some(path) => {
                let path = key(path, insensitive);
                path.starts_with(&data) && !own.iter().any(|folder| path.starts_with(folder))
            }
        };
        let embedded = self.embedded(&real_data, insensitive);
        let reached = self.paths.iter().chain(&embedded).any(|path| {
            resolve::resolve_all(path, &self.cwd, home)
                .iter()
                .any(inside)
        });
        // A search from a folder that holds `<data>` (or is it) reaches all of it.
        let holds = |found: &Option<PathBuf>| {
            inside(found)
                || found
                    .as_ref()
                    .is_some_and(|root| data.starts_with(&key(root, insensitive)))
        };
        reached
            || self.roots.iter().any(|root| {
                resolve::resolve_all(root, &self.cwd, home)
                    .iter()
                    .any(holds)
            })
    }

    /// The own folders that count ([`Self::allowed`], [`Self::cwd`]), as keys.
    fn own(&self, real_data: &Path, insensitive: bool) -> Vec<Vec<OsString>> {
        let home = self.home.as_deref();
        let data = key(real_data, insensitive);
        let mut own = Vec::new();
        let shared = SHARED.iter().map(|(place, _)| self.data.join(place));
        for folder in self.allowed.iter().cloned().chain(shared) {
            let Ok(place) = folder.strip_prefix(&self.data) else {
                continue;
            };
            let named = place
                .components()
                .all(|part| matches!(part, Component::Normal(_)));
            let expected = key(&real_data.join(place), insensitive);
            let found = resolve::resolve(&folder, &self.cwd, home);
            if named && found.is_some_and(|found| key(&found, insensitive) == expected) {
                own.push(expected);
            }
        }
        if let Some(cwd) = resolve::resolve(&self.cwd, &self.cwd, home) {
            let cwd_key = key(&cwd, insensitive);
            let linked = self.cwd.is_symlink() && cwd_key.starts_with(&data);
            if !data.starts_with(&cwd_key) && !linked {
                own.push(cwd_key);
            }
        }
        own
    }

    /// The paths that start, inside a shell word, where the data folder's text does: its path
    /// as configured and as resolved (each with `/` too on Windows) and its `~/…`, `$HOME/…` and
    /// `${HOME}/…` forms. Bash expands `$HOME` in double quotes; a single-quoted one is caught too.
    fn embedded(&self, real_data: &Path, insensitive: bool) -> Vec<PathBuf> {
        let mut needles: Vec<String> = Vec::new();
        for spelling in [self.data.as_path(), real_data] {
            needles.extend(spelling.to_str().map(str::to_owned));
            if let Some(home) = &self.home
                && let Ok(under) = spelling.strip_prefix(home)
                && let Some(under) = under.to_str()
            {
                needles.push(format!("~/{under}"));
                needles.push(format!("$HOME/{under}"));
                needles.push(format!("${{HOME}}/{under}"));
            }
        }
        if cfg!(windows) {
            let slashed: Vec<String> = needles.iter().map(|n| n.replace('\\', "/")).collect();
            needles.extend(slashed);
        }
        needles.dedup();
        self.words
            .iter()
            .flat_map(|word| command::embedded_paths(word, &needles, insensitive))
            .collect()
    }
}

/// What the matter thread found for a permission request's paths.
#[derive(Debug)]
pub(crate) enum PathsChecked {
    /// One resolves into Clauseo's data folder, outside the session's own folders.
    DataFolder,
    /// The first that a program holds open, if any (`clauseo_matters::held_open_at`).
    Held(Option<(MatterPath, HeldOpen)>),
}

/// The data-folder check, then the held-open check of `paths` in the matter `folder`.
pub(crate) fn check_paths(
    folder: &Path,
    paths: Vec<PathBuf>,
    fence: Option<&FenceCheck>,
) -> PathsChecked {
    if fence.is_some_and(FenceCheck::touches_data) {
        return PathsChecked::DataFolder;
    }
    PathsChecked::Held(
        paths
            .into_iter()
            .find_map(|path| held_open_at(folder, &path)),
    )
}

/// `path`'s parts for a prefix comparison, lowercased when `insensitive`.
fn key(path: &Path, insensitive: bool) -> Vec<OsString> {
    path.components()
        .map(|part| {
            let part = part.as_os_str();
            if insensitive {
                OsString::from(part.to_string_lossy().to_lowercase())
            } else {
                part.to_owned()
            }
        })
        .collect()
}

#[cfg(test)]
mod bypass_tests;
mod command;
#[cfg(test)]
mod command_tests;
mod input;
mod launch;
#[cfg(test)]
mod launch_tests;
mod resolve;
mod search;
#[cfg(test)]
mod search_tests;
#[cfg(test)]
mod tests;
