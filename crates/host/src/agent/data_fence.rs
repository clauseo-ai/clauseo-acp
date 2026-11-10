//! Clauseo's own data folder is off-limits to agents (decision record 0030, "The data folder
//! fence"). Under the jail (tiers A and B) stage C hides `<data>` outside the session's own
//! folders; with no sandbox (tier C) two lines stand in for it:
//!
//! - Claude's deny rules ([`claude_deny_rules`]): `Read(//…)` and `Edit(//…)` for each
//!   Clauseo-only subtree and file of `<data>` ([`DENIED`]) and, enumerated at each launch,
//!   every entry of `<data>` that is not the session's own (`launch.rs`: other matters' notes,
//!   other run folders, `run.old-*`, anything else), in the flag settings of every Claude
//!   session, and `Edit(//…)` alone, with a `Read` allow, for what every session may read and
//!   run ([`READ_ONLY`]: the agents' commands, Clauseo's Python runtime). Claude Code applies
//!   them to its file tools and to the file commands it recognises in Bash, never to the
//!   processes those start (`agent-browser`, `python3` read what they need). A deny rule has no
//!   exceptions, so the session's own run folder, this matter's notes
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

use clauseo_matters::{HeldOpen, held_open_resolved};
use clauseo_protocol::MatterPath;
use identity::DataIdentity;
use resolve::{Disk, Found};

use crate::network_paths::resolve::Entry;
use crate::network_paths::{FOLD_CASE, NetEnv, fold_name, refused_windows_name, touchable};
pub(crate) use health::DataFenceHealth;
pub(crate) use input::{candidates, candidates_roots, candidates_words};
pub(crate) use launch::{LaunchDeny, claude_projects, launch_deny};

/// What of `<data>` only Clauseo uses, named whether or not it exists at the launch (the
/// launch adds every other entry that exists then, `launch.rs`), as rule patterns under it,
/// each denied to Claude for Read and Edit: the skill packs' sources, the version store, the
/// logs, the updater's downloads, the usage counts, the Windows sandbox's ledger, the limits
/// reader's state, the browser's config, the block list's store and the lawyer's allowed sites, the
/// settings file, the updater's marker, the counts' first-act marker, the host's store, the search
/// index (every matter's prompts and answers). Not the built-in plugin ([`SHARED`]).
/// Under `agents`: Codex's home, the installs, the runtime and npm folders, the sign-in shim and
/// markers, and in Claude's own home its login, its settings, its transcripts (every project's,
/// with its subagents' and its memory; not the whole of `projects`, which also holds the large
/// tool results the model must Read back, `launch.rs`), its skills and
/// plugins, and what it keeps across sessions, so across matters: the prompt history, the
/// edited files' copies, the to-do lists and the shell snapshots. Not Claude's home whole:
/// Claude Code writes its plan note to `plans` there with its own Edit tool, and a deny rule
/// has no exceptions. And in every run folder, the own one too, the case-law relay's token file
/// (R24): `clauseo-mcp` reads it itself, so Claude's tools never need it, and a token read
/// through them would land in the transcript; and the shell browser's config file
/// (`browser::SESSION_CONFIG`), which agent-browser reads itself: an edit to it could set what
/// the `agent-browser` deny rules refuse as flags (`--profile`, `--config`).
/// Under `agents/runtime`, Node and the half-unpacked `.tmp-` folders, but not Clauseo's Python
/// runtime ([`READ_ONLY`]); the launch denies every other entry there that exists.
pub(crate) const DENIED: [&str; 38] = [
    "run/*/relay-token",
    "run/*/agent-browser/config.json",
    "skill-packs/**",
    "matters/**",
    "logs/**",
    "updates/**",
    "counts/**",
    "sandbox/**",
    "limits/**",
    "browser/**",
    "blocklist/**",
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
    "agents/runtime/node-v*/**",
    "agents/runtime/.tmp-*/**",
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

/// The prefix of the folder name of each build of Clauseo's Python runtime under
/// `<data>/agents/runtime` (`clauseo_agents`' `python.rs`, `FOLDER_PREFIX`):
/// `clauseo-python-3.13.18-c2-macos-arm64`, one per pinned build.
pub(crate) const PYTHON_RUNTIME_PREFIX: &str = "clauseo-python-";

/// What of `<data>` every session may read and run, never write, as rule patterns of folders
/// under it (a final `*` is any rest of the name): the commands the app copies for the agents
/// (`<data>/tools`, the shell browser's `agent-browser`), and each build of Clauseo's Python
/// runtime (its `bin/python3`, `pip` and `pandoc`, which read their own library there), the
/// same for every matter. Claude gets `Edit` denied ([`claude_deny_rules`]) and `Read` allowed
/// ([`claude_allow_rules`]) for each and all under it; the launch's enumeration leaves out
/// each real folder that fits, and denies its siblings (`launch.rs`). Codex's profile reaches
/// them without a rule (`agent::codex_fence`, `RuntimeDeny`). The host's auto-deny still
/// refuses a request that names them: neither agent needs to.
pub(crate) const READ_ONLY: [&str; 2] = ["tools", "agents/runtime/clauseo-python-*"];

/// Whether `name` is a build of Clauseo's Python runtime's folder name ([`PYTHON_RUNTIME_PREFIX`]
/// and something after it).
pub(crate) fn is_python_runtime(name: &str) -> bool {
    name.len() > PYTHON_RUNTIME_PREFIX.len() && name.starts_with(PYTHON_RUNTIME_PREFIX)
}

/// Claude's `permissions.deny` rules for a launch: for the data folder spelled each way in
/// `deny.spellings` (its canonical path first, then the path as configured when that differs),
/// `Read` and `Edit` for each of `deny.targets`, then `Edit` alone for each of [`READ_ONLY`]
/// and all under it. A spelling that is not UTF-8 gets none (no rule could name it).
pub(crate) fn claude_deny_rules(deny: &LaunchDeny) -> Vec<String> {
    let mut rules = Vec::new();
    let mut push = |rule: String| {
        if !rules.contains(&rule) {
            rules.push(rule);
        }
    };
    for spelling in &deny.spellings {
        let Some(base) = spelling.to_str().map(|text| rule_path(text, cfg!(windows))) else {
            continue;
        };
        for target in &deny.targets {
            for tool in ["Read", "Edit"] {
                push(format!("{tool}({base}/{target})"));
            }
        }
        for place in READ_ONLY {
            push(format!("Edit({base}/{place}/**)"));
        }
    }
    rules
}

/// The built-in plugin's skills folder under `<data>` (`clauseo_profile`'s `claude_skills`):
/// Clauseo's own skill assets, the same for every matter, which its skills send the model to
/// Read (legal-drafting's `section-checklists.md`).
pub(crate) const BUILT_IN_SKILLS: [&str; 3] = ["profile", "claude-plugin", "skills"];

/// Claude's `permissions.allow` rules for a launch: `Read` of the built-in skills' folder
/// ([`BUILT_IN_SKILLS`]) and all under it, so a built-in skill never asks the lawyer, then of
/// each of [`READ_ONLY`] and all under it, for each spelling of `<data>`. Read only, and
/// nothing wider; a deny rule (another session's, a link's) still beats each. A spelling that is
/// not UTF-8 gets none.
pub(crate) fn claude_allow_rules(deny: &LaunchDeny) -> Vec<String> {
    let skills: Vec<String> = BUILT_IN_SKILLS.iter().map(|name| escape(name)).collect();
    let skills = skills.join("/");
    let mut rules = Vec::new();
    for spelling in &deny.spellings {
        let Some(base) = spelling.to_str().map(|text| rule_path(text, cfg!(windows))) else {
            continue;
        };
        let places = std::iter::once(skills.as_str()).chain(READ_ONLY);
        for place in places {
            let rule = format!("Read({base}/{place}/**)");
            if !rules.contains(&rule) {
                rules.push(rule);
            }
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
    /// The host's data-fence health: the data folder's identity kept once read, retried while
    /// it can't be, and whether the fence compares by names only.
    pub(crate) health: DataFenceHealth,
}

impl FenceCheck {
    /// Whether any path resolves into `<data>` and outside every own folder, comparing ignoring
    /// case on Windows and macOS, whose file systems do. Stats the file system.
    #[cfg(test)]
    pub(crate) fn touches_data(&self) -> bool {
        self.touches_data_as(FOLD_CASE)
    }

    /// [`Self::touches_data`], ignoring case when `insensitive`. A path that passes through a
    /// link loop counts as in `<data>`, as does every path when `<data>` itself does not
    /// resolve.
    #[cfg(test)]
    pub(crate) fn touches_data_as(&self, insensitive: bool) -> bool {
        let net = NetEnv::current();
        self.touches_data_on(insensitive, &Disk::real(&net))
    }

    /// [`Self::touches_data_as`] on `disk`. A path that is, or leads to, a network location
    /// counts as in `<data>` (found before anything touches it), unless it lies by its text in
    /// the matter folder the lawyer keeps on a share, which counts as outside, untouched. A data
    /// folder that is itself on the network (redirected app data) is never touched for its
    /// identity: a path that resolves locally can't be in it, and every network path is refused.
    /// A path whose identity can't be read is refused; the data folder's own identity, when it
    /// can't be read, is compared by text alone, and [`Self::health`] says so
    /// ([`DataIdentity`]).
    pub(crate) fn touches_data_on(&self, insensitive: bool, disk: &Disk<'_>) -> bool {
        let anything = !self.paths.is_empty() || !self.words.is_empty() || !self.roots.is_empty();
        let home = self.home.as_deref();
        let network_data = disk.net.is_network(&self.data);
        let real_data = if network_data {
            self.data.clone()
        } else {
            match resolve::resolve(&self.data, &self.cwd, home, disk) {
                Found::Path(real_data) => real_data,
                Found::Refused | Found::ShareMatter => return anything,
            }
        };
        let identity = if network_data {
            DataIdentity::none(disk.fs)
        } else {
            DataIdentity::capture(disk.fs, &real_data, &self.health)
        };
        let data = key(&real_data, insensitive);
        let own = if network_data {
            Vec::new()
        } else {
            self.own(&real_data, insensitive, disk, &identity)
        };
        let inside = |found: &Found| match found {
            Found::Refused => true,
            Found::ShareMatter => false,
            Found::Path(_) if network_data => false,
            Found::Path(path) => {
                let text = key(path, insensitive);
                if text.starts_with(&data) {
                    !own.iter().any(|folder| text.starts_with(folder))
                } else {
                    identity.holds(path)
                }
            }
        };
        // An agent's text with a name Windows reads as another place is refused untouched.
        let agent = |path: &PathBuf| {
            if refused_windows_name(path, disk.net) {
                vec![Found::Refused]
            } else {
                resolve::resolve_all(path, &self.cwd, home, disk)
            }
        };
        let embedded = self.embedded(&real_data, !network_data, insensitive, disk.net.windows);
        let reached = self
            .paths
            .iter()
            .chain(&embedded)
            .any(|path| agent(path).iter().any(inside));
        // A search from a folder that holds `<data>` (or is it) reaches all of it.
        let holds = |found: &Found| {
            inside(found)
                || matches!(found, Found::Path(root) if !network_data
                    && (data.starts_with(&key(root, insensitive)) || identity.is_on_line(root)))
        };
        reached || self.roots.iter().any(|root| agent(root).iter().any(holds))
    }

    /// The own folders that count ([`Self::allowed`], [`Self::cwd`]), as keys.
    fn own(
        &self,
        real_data: &Path,
        insensitive: bool,
        disk: &Disk<'_>,
        identity: &DataIdentity<'_>,
    ) -> Vec<Vec<OsString>> {
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
            let found = resolve::resolve(&folder, &self.cwd, home, disk);
            if named && matches!(found, Found::Path(found) if key(&found, insensitive) == expected)
            {
                own.push(expected);
            }
        }
        if let Found::Path(cwd) = resolve::resolve(&self.cwd, &self.cwd, home, disk) {
            let cwd_key = key(&cwd, insensitive);
            // A matter folder that holds `<data>` by identity (another spelling of one of its
            // folders) counts for nothing either, nor one whose identity can't be read.
            let holds_by_identity = identity.is_on_line(&cwd);
            let linked = matches!(disk.fs.entry(&self.cwd), Ok(Entry::Link))
                && (cwd_key.starts_with(&data) || identity.holds(&cwd));
            if !data.starts_with(&cwd_key) && !holds_by_identity && !linked {
                own.push(cwd_key);
            }
        }
        own
    }

    /// The paths that start, inside a shell word, where one of the data folder's spellings does
    /// ([`words::needles`]: as configured and as resolved, its short name when `local`, its
    /// home and folder variable forms), each checked as the data folder's path followed by the
    /// rest of the word. Bash expands `$HOME` in double quotes; a single-quoted one is caught
    /// too.
    fn embedded(
        &self,
        real_data: &Path,
        local: bool,
        insensitive: bool,
        windows: bool,
    ) -> Vec<PathBuf> {
        let var = |name: &str| std::env::var_os(name).map(PathBuf::from);
        let needles = words::needles(&words::Spellings {
            paths: [self.data.as_path(), real_data],
            short: local.then(|| words::short_name(real_data)).flatten(),
            home: self.home.as_deref(),
            var: &var,
            windows,
        });
        self.words
            .iter()
            .flat_map(|word| {
                command::embedded_paths(word, &needles, real_data, insensitive, windows)
            })
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

/// The data-folder check, then the held-open check of `paths` in the matter `folder`. A path
/// that is or leads to a network location, or lies by its text in a matter folder on a share,
/// is never looked at for being held; one that may be is looked at by the path the network-safe
/// walk resolved, never by its text again ([`touchable`]).
pub(crate) fn check_paths(
    folder: &Path,
    paths: Vec<PathBuf>,
    fence: Option<&FenceCheck>,
) -> PathsChecked {
    let net = NetEnv::current();
    let disk = Disk::real(&net);
    if fence.is_some_and(|fence| fence.touches_data_on(FOLD_CASE, &disk)) {
        return PathsChecked::DataFolder;
    }
    let walk = Disk::resolved(&net);
    PathsChecked::Held(
        paths
            .into_iter()
            .filter_map(|path| touchable(&path, folder, &walk))
            .find_map(|walked| held_open_resolved(folder, &walked)),
    )
}

/// `path`'s parts for a prefix comparison, folded when `insensitive` ([`fold_name`]: a deny
/// folds at least as broadly as the file system, `ſ` and the Kelvin sign included).
fn key(path: &Path, insensitive: bool) -> Vec<OsString> {
    path.components()
        .map(|part| {
            let part = part.as_os_str();
            if insensitive {
                OsString::from(fold_name(&part.to_string_lossy()))
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
#[cfg(test)]
mod drive_word_tests;
mod health;
mod identity;
mod input;
mod launch;
#[cfg(test)]
mod launch_tests;
#[cfg(test)]
mod names_only_tests;
#[cfg(test)]
mod network_forms_tests;
#[cfg(test)]
mod network_tests;
mod resolve;
mod search;
#[cfg(test)]
mod search_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
#[cfg(windows)]
mod windows_names_only_tests;
#[cfg(test)]
#[cfg(windows)]
mod windows_real_tests;
#[cfg(test)]
mod windows_tests;
mod words;
