//! Claude Code's own sandbox for a Claude session on macOS (and Linux, for development;
//! decision record 0030, "Claude's sandbox"): the `sandbox` object of the flag settings, which
//! runs each Bash command under Seatbelt (bubblewrap on Linux) and lets a sandboxed command run
//! without asking (`autoAllowBashIfSandboxed`). Windows gets none ([`ClaudeSandbox::for_os`]):
//! Claude Code 2.1.293 never auto-allows there (`autoAllowBashIfSandboxed` reads `false` on
//! Windows), and its Windows sandbox needs a one-time admin install behind a feature gate, so
//! commands keep asking.
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
//! - `network.allowedDomains: []`: a sandboxed command reaching a new host asks; the seeded
//!   `WebFetch(domain:…)` allow rules widen it to those sites.
//! - `filesystem.allowWrite`: the matter folder, the work folder, the session's temp folder and
//!   the matter's notes (Claude Code adds its own temp folder, `CLAUDE_CODE_TMPDIR`'s, itself).
//! - `filesystem.denyRead: ["~/"]`: the lawyer's home (other clients' files, `~/.ssh`), where
//!   `<data>` also lies; the data fence's `Read(…)` deny rules become read denies too.
//! - `filesystem.allowRead` ([`ClaudeSandbox::allow_read`]): re-opens, within those denies,
//!   what a sandboxed command needs. Not the whole run folder: it holds the case-law relay's
//!   token and the browser's profile (the visited sites' cookies).
//! - `ignoreViolations`: Claude Code's shell snapshot folder in its home, which the data fence
//!   denies. Each command starts with `source <snapshot> 2>/dev/null || true`, so a denied read
//!   costs only the lawyer's shell functions and aliases, never the command (the snapshot's
//!   `PATH` is the launch's own); the report of it is left out so the model is not told of a
//!   violation on every command.
//!
//! Paths go in as UTF-8 text; one that is not is left out, so it is neither readable nor
//! writable in the sandbox (it fails closed).

use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

/// The command patterns that run outside the sandbox in a session with the browser.
pub(crate) const EXCLUDED_COMMANDS: [&str; 1] = ["agent-browser:*"];

/// The home folder denied to sandboxed commands, as Claude Code expands it.
pub(crate) const DENY_READ: [&str; 1] = ["~/"];

/// Claude Code's shell snapshot folder in its config folder.
const SNAPSHOTS: &str = "shell-snapshots";

/// What a Claude session's sandbox opens, from [`SandboxPaths`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ClaudeSandbox {
    /// `filesystem.allowWrite`.
    pub(crate) allow_write: Vec<String>,
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
    /// `<run>/downloads`, the browser's downloads.
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
    /// The session has the browser as a shell tool.
    pub(crate) browser: bool,
}

/// The prefix of the Python runtime's folder name (`agents::python`).
const RUNTIME_PREFIX: &str = "clauseo-python-";

impl ClaudeSandbox {
    /// The sandbox for a session on Windows (`windows`) or not: none on Windows.
    pub(crate) fn for_os(windows: bool, paths: &SandboxPaths<'_>) -> Option<Self> {
        (!windows).then(|| Self::of(paths))
    }

    /// The sandbox for `paths`.
    pub(crate) fn of(paths: &SandboxPaths<'_>) -> Self {
        let allow_write = texts([paths.matter, paths.work, paths.tmp, paths.notes]);
        Self {
            allow_write,
            allow_read: Self::allow_read(paths),
            quiet: texts([paths.claude_home.join(SNAPSHOTS).as_path()]),
            browser: paths.browser,
        }
    }

    /// `filesystem.allowRead`: the writable folders, the browser's downloads, the skill packs,
    /// the built-in skills, and each folder first on `PATH` (stetdoc's), except that a folder
    /// in Clauseo's Python runtime (`<rt>/bin`) opens the whole runtime, whose interpreter reads
    /// its standard library and site-packages beside it. Never a `PATH` folder's parent
    /// otherwise: it could be the home itself.
    fn allow_read(paths: &SandboxPaths<'_>) -> Vec<String> {
        let mut folders: Vec<PathBuf> = [
            paths.matter,
            paths.work,
            paths.tmp,
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
                "denyRead": DENY_READ,
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
