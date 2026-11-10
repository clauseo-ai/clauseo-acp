//! Claude Code's own sandbox for a Claude session on macOS (and Linux, for development;
//! decision record 0030, "Claude's sandbox"): the `sandbox` object of the flag settings, which
//! runs each Bash command under Seatbelt (bubblewrap on Linux) and lets a sandboxed command run
//! without asking (`autoAllowBashIfSandboxed`). A session gets none ([`ClaudeSandbox::for_os`]),
//! so its commands keep asking:
//!
//! - on Windows: Claude Code 2.1.293 never auto-allows there, and its Windows sandbox needs a
//!   one-time admin install behind a feature gate;
//! - when Claude's short temp folder could not be made (`agent::temp::make_claude_tmp`);
//! - when a path in its lists holds a glob character (`*`, `?`, `[`, `]`): Claude Code reads
//!   sandbox paths as patterns and does not escape them, so `Smith [2024]` would open `Smith 2`;
//! - when a path it must deny is not UTF-8 (it could not be named).
//!
//! What the object says, every key Clauseo's:
//!
//! - `enabled`, `autoAllowBashIfSandboxed`: on. Forced ask turns the second off after this
//!   (`profile::forced_ask_settings`), so a forced-ask session asks for every command, sandboxed
//!   or not.
//! - `allowUnsandboxedCommands: false`: the model's `dangerouslyDisableSandbox` is ignored, so
//!   no command breaks out, asked or not.
//! - `failIfUnavailable: false`: a Mac where the sandbox can't start keeps the session, and its
//!   commands ask, since Claude Code auto-allows only while sandboxing is running (2.1.293 checks
//!   `isSandboxingEnabled()` before auto-allowing; a sandbox that fails to start for one command
//!   refuses it and is off for the rest of the session). `true` would end Claude at start-up.
//! - `allowAppleEvents: false`: `open` and `osascript` can't launch or drive other apps. With
//!   the Spotlight and Quick Look services outside the sandbox's Mach list, `mdls`, `mdfind` and
//!   `qlmanage` fail too: the accepted breakages, which Claude is told of on macOS
//!   (`clauseo_profile::instructions::claude_shell`).
//! - `excludedCommands: ["agent-browser:*"]`, only in a session with the browser: agent-browser
//!   needs its daemon, Chrome and the session's sockets. Claude Code 2.1.293 runs a command
//!   outside the sandbox only when every part of it matches (`commands.every(...)`), and a
//!   command it can't split into plain parts (a substitution, a redirect to a file, a backtick,
//!   a parse error) stays sandboxed, so `agent-browser snapshot && touch ~/x` runs sandboxed. An
//!   excluded command still goes through the permission rules.
//! - `network.allowedDomains: []`: a sandboxed command reaching a new host asks (the seeded
//!   `WebFetch(domain:…)` sites probably ask too: the list is built from trusted settings only).
//! - `filesystem.allowWrite`: the matter folder, the work folder, the session's temp folder, the
//!   matter's notes and Claude's short temp folder (`CLAUDE_CODE_TMPDIR`, where sandboxed
//!   commands' `TMPDIR` is made).
//! - `filesystem.denyWrite`: the matter's `.clauseo` folder, which holds `matter.json`, the
//!   marker that finds a moved matter folder again.
//! - `filesystem.denyRead`: the lawyer's home (other clients' files, `~/.ssh`), each spelling of
//!   `<data>` (it may lie outside the home), every other matter's folder the host knows at the
//!   launch, `/Volumes` and `/Users/Shared`; the data fence's `Read(…)` deny rules become read
//!   denies too. A matter registered after the launch is in no list: the accepted residual.
//! - `filesystem.allowRead` ([`ClaudeSandbox::allow_read`]): re-opens, within those denies,
//!   what a sandboxed command needs. Not the whole run folder: it holds the case-law relay's
//!   token and the browser's profile (the visited sites' cookies).
//! - `ignoreViolations`: Claude Code's shell snapshot folder in its home, which the data fence
//!   denies. Each command starts with `source <snapshot> 2>/dev/null || true`, so a denied read
//!   costs only the lawyer's shell functions and aliases, never the command (the snapshot's
//!   `PATH` is the launch's own); the report of it is left out so the model is not told of a
//!   violation on every command.
//!
//! Paths go in as UTF-8 text. An allowed one that is not is left out, so it is neither readable
//! nor writable in the sandbox (it fails closed).

use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

/// The command patterns that run outside the sandbox in a session with the browser.
pub(crate) const EXCLUDED_COMMANDS: [&str; 1] = ["agent-browser:*"];

/// What sandboxed commands may not read beside `<data>` and the other matters: the home, as
/// Claude Code expands it, and macOS's shared places where matters may be kept.
pub(crate) const DENY_READ: [&str; 3] = ["~/", "/Volumes", "/Users/Shared"];

/// The characters Claude Code reads as a pattern in a sandbox path.
const GLOB: [char; 4] = ['*', '?', '[', ']'];

/// Claude Code's shell snapshot folder in its config folder.
const SNAPSHOTS: &str = "shell-snapshots";

/// The matter folder's own Clauseo folder (`clauseo_matters`' `.clauseo/matter.json`).
pub(crate) const MATTER_MARKER: &str = ".clauseo";

/// What a Claude session's sandbox opens and closes, from [`SandboxPaths`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ClaudeSandbox {
    /// `filesystem.allowWrite`.
    pub(crate) allow_write: Vec<String>,
    /// `filesystem.denyWrite`.
    pub(crate) deny_write: Vec<String>,
    /// `filesystem.denyRead`.
    pub(crate) deny_read: Vec<String>,
    /// `filesystem.allowRead`.
    pub(crate) allow_read: Vec<String>,
    /// The paths whose violations go unreported, for every command.
    pub(crate) quiet: Vec<String>,
    /// The session has the browser: `excludedCommands` holds [`EXCLUDED_COMMANDS`].
    pub(crate) browser: bool,
}

/// The session's paths the sandbox names.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SandboxPaths<'a> {
    /// The matter folder (the agent's working folder).
    pub(crate) matter: &'a Path,
    /// `<run>/work`.
    pub(crate) work: &'a Path,
    /// `<run>/tmp` (`agent::temp`).
    pub(crate) tmp: &'a Path,
    /// Claude's short temp folder (`agent::temp::claude_tmp_dir`), if it was made.
    pub(crate) claude_tmp: Option<&'a Path>,
    /// `<run>/agent-browser/downloads`, the browser's downloads.
    pub(crate) downloads: &'a Path,
    /// `<run>/skill-packs`, the launch's skill packs.
    pub(crate) skill_packs: &'a Path,
    /// The matter's notes.
    pub(crate) notes: &'a Path,
    /// The built-in plugin's skills, `<data>/profile/claude-plugin/skills`, for each spelling
    /// of `<data>`.
    pub(crate) skills: &'a [PathBuf],
    /// The folders first on the agent's `PATH`: stetdoc's, then the profile hook's (Clauseo's
    /// Python runtime, `<rt>/bin`, where pandoc is too).
    pub(crate) path: &'a [PathBuf],
    /// Claude Code's config folder (`agent::home`).
    pub(crate) claude_home: &'a Path,
    /// Each spelling of `<data>` (`LaunchDeny::spellings`).
    pub(crate) data: &'a [PathBuf],
    /// Every other matter's folder the host knows.
    pub(crate) other_matters: &'a [PathBuf],
    /// The session has the browser as a shell tool.
    pub(crate) browser: bool,
}

/// The prefix of the Python runtime's folder name (`agents::python`).
const RUNTIME_PREFIX: &str = "clauseo-python-";

impl ClaudeSandbox {
    /// The sandbox for a session on Windows (`windows`) or not: none on Windows, nor when
    /// [`Self::of`] gives none.
    pub(crate) fn for_os(windows: bool, paths: &SandboxPaths<'_>) -> Option<Self> {
        if windows {
            return None;
        }
        Self::of(paths)
    }

    /// The sandbox for `paths`; none without Claude's short temp folder, when a path to deny is
    /// not UTF-8, or when any path holds a glob character (one info line, no path).
    pub(crate) fn of(paths: &SandboxPaths<'_>) -> Option<Self> {
        let Some(claude_tmp) = paths.claude_tmp else {
            tracing::info!("Claude's sandbox is off: its temp folder was not made");
            return None;
        };
        let denied = paths.data.iter().chain(paths.other_matters);
        let Some(mut deny_read) = strict_texts(denied.map(PathBuf::as_path)) else {
            tracing::info!("Claude's sandbox is off: a folder to deny is not UTF-8");
            return None;
        };
        deny_read.splice(0..0, DENY_READ.iter().map(|path| (*path).to_owned()));
        let marker = paths.matter.join(MATTER_MARKER);
        let sandbox = Self {
            allow_write: texts([paths.matter, paths.work, paths.tmp, paths.notes, claude_tmp]),
            deny_write: texts([marker.as_path()]),
            deny_read,
            allow_read: Self::allow_read(paths, claude_tmp),
            quiet: texts([paths.claude_home.join(SNAPSHOTS).as_path()]),
            browser: paths.browser,
        };
        if sandbox.has_glob() {
            tracing::info!("Claude's sandbox is off: a sandbox path holds a glob character");
            return None;
        }
        Some(sandbox)
    }

    /// Whether any path in the lists holds a [`GLOB`] character.
    fn has_glob(&self) -> bool {
        [
            &self.allow_write,
            &self.deny_write,
            &self.deny_read,
            &self.allow_read,
            &self.quiet,
        ]
        .into_iter()
        .flatten()
        .any(|path| path.contains(GLOB))
    }

    /// `filesystem.allowRead`: the writable folders, the browser's downloads, the skill packs,
    /// the built-in skills, and each folder first on `PATH` (stetdoc's), except that a folder
    /// in Clauseo's Python runtime (`<rt>/bin`) opens the whole runtime, whose interpreter reads
    /// its standard library and site-packages beside it. Never a `PATH` folder's parent
    /// otherwise: it could be the home itself.
    fn allow_read(paths: &SandboxPaths<'_>, claude_tmp: &Path) -> Vec<String> {
        let mut folders: Vec<PathBuf> = [
            paths.matter,
            paths.work,
            paths.tmp,
            claude_tmp,
            paths.downloads,
            paths.skill_packs,
            paths.notes,
        ]
        .iter()
        .map(|path| path.to_path_buf())
        .collect();
        folders.extend(paths.skills.iter().cloned());
        for folder in paths.path {
            let runtime = folder.parent().filter(|parent| {
                parent
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with(RUNTIME_PREFIX))
            });
            folders.push(runtime.unwrap_or(folder).to_path_buf());
        }
        texts(folders.iter().map(PathBuf::as_path))
    }

    /// The `sandbox` object (see the module doc).
    pub(crate) fn settings(&self) -> Value {
        let mut sandbox = json!({
            "enabled": true,
            "autoAllowBashIfSandboxed": true,
            "allowUnsandboxedCommands": false,
            "failIfUnavailable": false,
            "allowAppleEvents": false,
            "network": {"allowedDomains": []},
            "filesystem": {
                "allowWrite": self.allow_write,
                "denyWrite": self.deny_write,
                "denyRead": self.deny_read,
                "allowRead": self.allow_read,
            },
        });
        if let Some(object) = sandbox.as_object_mut() {
            if self.browser {
                object.insert("excludedCommands".to_owned(), json!(EXCLUDED_COMMANDS));
            }
            if !self.quiet.is_empty() {
                object.insert("ignoreViolations".to_owned(), json!({"*": self.quiet}));
            }
        }
        sandbox
    }
}

/// Puts `sandbox`'s object in `settings`, in place of any `sandbox` the hook sent.
pub(crate) fn sandbox_settings(settings: &mut Map<String, Value>, sandbox: &ClaudeSandbox) {
    settings.insert("sandbox".to_owned(), sandbox.settings());
}

/// The paths as UTF-8 text, in order, each once; one that is not UTF-8 is left out.
fn texts<'a>(paths: impl IntoIterator<Item = &'a Path>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for text in paths.into_iter().filter_map(Path::to_str) {
        if !out.iter().any(|seen| seen == text) {
            out.push(text.to_owned());
        }
    }
    out
}

/// [`texts`], or none when a path is not UTF-8.
fn strict_texts<'a>(paths: impl IntoIterator<Item = &'a Path>) -> Option<Vec<String>> {
    let paths: Vec<&Path> = paths.into_iter().collect();
    paths
        .iter()
        .all(|path| path.to_str().is_some())
        .then(|| texts(paths))
}
