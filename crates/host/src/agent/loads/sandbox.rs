//! Claude Code's own sandbox for a Claude session on macOS (and Linux, for development;
//! decision record 0030, "Claude's sandbox"): the `sandbox` object of the flag settings, which
//! runs each Bash command under Seatbelt (bubblewrap on Linux) and lets a sandboxed command run
//! without asking (`autoAllowBashIfSandboxed`). A session gets none ([`ClaudeSandbox::for_os`],
//! [`Off`]), so its commands go to the mode (`auto`'s classifier):
//!
//! - on Windows: Claude Code 2.1.293 never auto-allows there, and its Windows sandbox needs a
//!   one-time admin install behind a feature gate;
//! - when Claude's short temp folder could not be made (`agent::temp::make_claude_tmp`);
//! - when a path in its lists holds `*` or `?`, which Claude Code reads as a pattern with no
//!   escape ([`Off::Wildcard`]); that session also leaves `auto` for `acceptEdits`
//!   (`actor/turn/session/claude_mode.rs`, `Auto::NoSandbox`), so no command runs on the
//!   classifier alone where the sandbox was meant to be;
//! - when a path it must deny is not UTF-8 (it could not be named).
//!
//! A `[` (in `Smith [2024]`) is escaped instead ([`escape_brackets`]). Claude Code 2.1.293
//! reads a sandbox path holding `*`, `?`, `[` or `]` as a glob (its `W4t`: `[…]` stays a
//! regex class, `*` becomes `[^/]*` even inside one, a `\` is a literal backslash, so no
//! escape exists for `*` or `?`). `[[]` is a class holding only `[`, and a lone `]` is literal,
//! so `Smith [[]2024]` names `Smith [2024]` alone. A glob deny covers all under it (`z4t`
//! adds `(/.*)?`), but a glob allow only the path itself (`W4t`, Seatbelt's `(regex …)`), so
//! each escaped allowed folder goes in twice, as itself and with `/**`. Which lists read globs
//! differs by OS (code.claude.com/docs/en/settings-reference, "Sandbox path prefixes"): every
//! list on macOS, only `denyRead` and `allowRead` on Linux, where a glob write path is
//! skipped, so the write lists stay as written there.
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
//! - `network.allowedDomains: ["*"]`: every host, so a sandboxed command (`curl` of a court PDF)
//!   never asks for a domain; Claude Code 2.1.293 accepts it (stage C, 2026-11-11: a real
//!   judgment PDF downloaded with no prompt and no certificate error in auto mode).
//!   `network.deniedDomains` keeps loopback out even through Claude Code's own proxy, which runs
//!   outside the sandbox: the browser's debugging port and agent-browser's daemon listen there.
//!   A direct loopback connect is refused by the sandbox already (stage C, the same day).
//!   `0.0.0.0` and `[::]` are denied too: a connect to the any-address lands on loopback
//!   (stage C on a Mac) ([`DENIED_DOMAINS`]).
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
//!   what a sandboxed command needs, with the `PATH` file Claude Code reads into each command
//!   (`<run>/clauseo-env.sh`, `agent::shell_path`), the trust roots file on macOS
//!   (`<data>/tls/roots.pem`, [`super::tls_roots`], the file alone) and the shell browser's
//!   tools folder. Not
//!   the whole run folder: it holds the case-law relay's
//!   token and the browser's profile (the visited sites' cookies). Of Claude's own project
//!   folder only `<home>/projects/<key>/*/tool-results`, where `WebFetch` keeps the files it
//!   fetches (a PDF among them): never the transcripts or the memory beside them.
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

/// The hosts a sandboxed command may not reach even through Claude Code's proxy: loopback, where
/// the browser's debugging port and agent-browser's daemon listen, and the any-address
/// `0.0.0.0` and `[::]`, which reach loopback too (stage C on a Mac, 2026-11-11). `127.1`, the
/// decimal forms and `[::ffff:127.0.0.1]` come out as `127.0.0.1` once the proxy reads them.
pub(crate) const DENIED_DOMAINS: [&str; 6] =
    ["localhost", "127.0.0.1", "::1", "[::1]", "0.0.0.0", "[::]"];

/// The command patterns that run outside the sandbox in a session with the browser.
pub(crate) const EXCLUDED_COMMANDS: [&str; 1] = ["agent-browser:*"];

/// What sandboxed commands may not read beside `<data>` and the other matters: the home, as
/// Claude Code expands it, and macOS's shared places where matters may be kept.
pub(crate) const DENY_READ: [&str; 3] = ["~/", "/Volumes", "/Users/Shared"];

/// The characters Claude Code reads as a pattern in a sandbox path that no escape makes literal.
const WILDCARDS: [char; 2] = ['*', '?'];

/// The OS a sandbox is built for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SandboxOs {
    /// Seatbelt: every list reads globs.
    Mac,
    /// bubblewrap, for development: only the read lists read globs.
    Linux,
    /// No sandbox.
    Windows,
}

impl SandboxOs {
    /// The OS this build runs on.
    pub(crate) const CURRENT: Self = if cfg!(windows) {
        Self::Windows
    } else if cfg!(target_os = "macos") {
        Self::Mac
    } else {
        Self::Linux
    };
}

/// Why a session gets no sandbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Off {
    /// Windows (see the module doc).
    Windows,
    /// Claude's short temp folder was not made.
    NoTemp,
    /// A path to deny is not UTF-8.
    NotUtf8,
    /// A path holds `*` or `?`: the session also runs in `acceptEdits`, not `auto`.
    Wildcard,
}

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
    /// The folders first on the agent's `PATH` and its shells' (`agent::shell_path`):
    /// stetdoc's, the shell browser's tools (`<data>/tools`) in a session that has them, then
    /// the profile hook's (Clauseo's Python runtime, `<rt>/bin`, where pandoc is too).
    pub(crate) path: &'a [PathBuf],
    /// The file that puts those folders first in each Bash command (`<run>/clauseo-env.sh`,
    /// `agent::shell_path::ENV_FILE`), if it was written: the file alone, never its folder.
    pub(crate) env_file: Option<&'a Path>,
    /// The trust roots file its CA variables name (`tls_roots`), if written: the file alone.
    pub(crate) ca_file: Option<&'a Path>,
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
    /// The sandbox for `paths` on `os`; none on Windows, without Claude's short temp folder,
    /// when a path to deny is not UTF-8, or when any path holds `*` or `?` (one info line, no
    /// path). A `[` is escaped ([`escape_brackets`]).
    pub(crate) fn for_os(os: SandboxOs, paths: &SandboxPaths<'_>) -> Result<Self, Off> {
        if os == SandboxOs::Windows {
            return Err(Off::Windows);
        }
        let Some(claude_tmp) = paths.claude_tmp else {
            tracing::info!("Claude's sandbox is off: its temp folder was not made");
            return Err(Off::NoTemp);
        };
        let denied = paths.data.iter().chain(paths.other_matters);
        let Some(mut deny_read) = strict_texts(denied.map(PathBuf::as_path)) else {
            tracing::info!("Claude's sandbox is off: a folder to deny is not UTF-8");
            return Err(Off::NotUtf8);
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
        if sandbox.has_wildcard() {
            tracing::info!("Claude's sandbox is off: a sandbox path holds * or ?; acceptEdits");
            return Err(Off::Wildcard);
        }
        let mut sandbox = sandbox.escaped(os);
        sandbox
            .allow_read
            .extend(tool_results(paths.claude_home, paths.matter));
        Ok(sandbox)
    }

    /// The lists with each `[` escaped where `os` reads globs, each escaped allowed path also
    /// with `/**` (see the module doc).
    fn escaped(self, os: SandboxOs) -> Self {
        let globs_in_writes = os == SandboxOs::Mac;
        let allowed = |list: Vec<String>| list.into_iter().flat_map(allowed_entries).collect();
        let denied = |list: Vec<String>| list.iter().map(|path| escape_brackets(path)).collect();
        Self {
            allow_write: if globs_in_writes {
                allowed(self.allow_write)
            } else {
                self.allow_write
            },
            deny_write: if globs_in_writes {
                denied(self.deny_write)
            } else {
                self.deny_write
            },
            deny_read: denied(self.deny_read),
            allow_read: allowed(self.allow_read),
            quiet: denied(self.quiet),
            browser: self.browser,
        }
    }

    /// Whether any path in the lists holds one of [`WILDCARDS`].
    fn has_wildcard(&self) -> bool {
        [
            &self.allow_write,
            &self.deny_write,
            &self.deny_read,
            &self.allow_read,
            &self.quiet,
        ]
        .into_iter()
        .flatten()
        .any(|path| path.contains(WILDCARDS))
    }

    /// `filesystem.allowRead`: the writable folders, the browser's downloads, the skill packs,
    /// the built-in skills, the shells' `PATH` file, the trust roots file, and each folder first
    /// on `PATH` (stetdoc's, the shell browser's tools), except that a folder in Clauseo's
    /// Python runtime (`<rt>/bin`) opens the whole runtime, whose interpreter reads
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
        folders.extend(paths.env_file.map(Path::to_path_buf));
        folders.extend(paths.ca_file.map(Path::to_path_buf));
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
            "network": {
                "allowedDomains": ["*"],
                "deniedDomains": DENIED_DOMAINS,
            },
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

/// The tool results of every session of this matter's project in Claude Code's config folder,
/// where `WebFetch` keeps the files it fetches: `<home>/projects/<key>/*/tool-results`, and the
/// same with `/**` (a glob allow opens only what it names), the key Claude Code's own
/// (`data_fence::claude_project_key`). Never the project folder itself, which holds the
/// session transcripts and the memory. None when a path is not UTF-8. The `*` stands for one
/// session's folder; Claude Code reads it after the first folder name, as Linux requires.
fn tool_results(home: &Path, matter: &Path) -> Vec<String> {
    let (Some(home), Some(matter)) = (home.to_str(), matter.to_str()) else {
        return Vec::new();
    };
    let key = crate::agent::data_fence::claude_project_key(matter);
    let base = format!("{}/projects/{key}/*/tool-results", escape_brackets(home));
    vec![base.clone(), format!("{base}/**")]
}

/// `text` with each `[` as `[[]`, a glob class holding only `[`; a lone `]` is literal already
/// (see the module doc).
pub(crate) fn escape_brackets(text: &str) -> String {
    text.replace('[', "[[]")
}

/// An allowed path's entries: itself, and with `/**` too once escaped (a glob allow opens only
/// what it names).
fn allowed_entries(path: String) -> Vec<String> {
    if path.contains('[') {
        let escaped = escape_brackets(&path);
        vec![escaped.clone(), format!("{escaped}/**")]
    } else {
        vec![path]
    }
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
