//! Claude Code's own sandbox for a Claude session on macOS (and Linux, for development;
//! decision record 0030, "Claude's sandbox"): the `sandbox` object of the flag settings, which
//! runs each Bash command under Seatbelt (bubblewrap on Linux) and lets a sandboxed command run
//! without asking (`autoAllowBashIfSandboxed`). There is none ([`ClaudeSandbox::for_os`],
//! [`Off`]):
//!
//! - on Windows, by design: Claude Code 2.1.293 never auto-allows there, and its Windows sandbox
//!   needs a one-time admin install behind a feature gate, so its commands go to the mode
//!   (`auto`'s classifier) under the guard hook;
//! - when Claude's short temp folder could not be made (`agent::temp::make_claude_tmp`);
//! - when a path of the session's own (or `<data>`) holds one of [`WILDCARDS`], which Claude
//!   Code reads as a pattern with no escape ([`Off::Wildcard`], with the folder holding it);
//! - when another matter's registered folder holding one would deny one of the session's own
//!   folders ([`Off::CoversOwn`]): on macOS its deny stays the pattern it is, which only ever
//!   covers more than the folder; on Linux it is widened to the folder above it with neither
//!   ([`glob`], where the runtime's conversion is quoted). Otherwise another matter's folder
//!   never turns the sandbox off;
//! - when a path it must deny is not UTF-8 (it could not be named): where another matter's
//!   folder leads, or `<data>`'s real path (a stored folder is always UTF-8).
//!
//! Each of those but Windows refuses the launch ([`ClaudeSandbox::for_launch`], [`Off::refusal`]:
//! `matter.folder_wildcard`, `agent.sandbox_wildcard`, `matter.name_unreadable`,
//! `agent.sandbox_unavailable`), with one warn line: Claude never starts on macOS or Linux
//! without its sandbox.
//!
//! A `[` (in `Smith [2024]`) is escaped instead ([`escape_brackets`]). Claude Code 2.1.293
//! reads a sandbox path holding `*`, `?`, `[` or `]` as a glob (its `W4t`, sandbox-runtime
//! 0.0.77/0.0.78's `globToRegex`: `[…]` stays a regex class, `*` becomes `[^/]*` even inside
//! one, a `\` is a literal backslash, so no escape exists for `*` or `?`). `[[]` is a class
//! holding only `[`, and a lone `]` is literal, so `Smith [[]2024]` names `Smith [2024]` alone.
//! A glob deny covers all under it (`z4t` adds `(/.*)?`), but a glob allow only the path itself
//! (`W4t`, Seatbelt's `(regex …)`), so each escaped allowed folder goes in twice, as itself and
//! with `/**`. Which lists read globs differs by OS
//! (code.claude.com/docs/en/settings-reference, "Sandbox path prefixes"): every
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
//! - `failIfUnavailable: true` (the security review of #940): a sandbox that fails at run time
//!   fails the command instead of running it unsandboxed, and a Mac (or a Linux machine without
//!   bubblewrap) where it can't start at all ends Claude at start-up, so the launch fails rather
//!   than run commands with no sandbox (the setting's own description). The host's own checks
//!   come first and refuse the launch (the list above).
//! - `allowAppleEvents: false`: `open` and `osascript` can't launch or drive other apps. With
//!   the Spotlight and Quick Look services outside the sandbox's Mach list, `mdls`, `mdfind` and
//!   `qlmanage` fail too: the accepted breakages, which Claude is told of on macOS
//!   (`clauseo_profile::instructions::claude_shell`).
//! - No `excludedCommands`: every command runs sandboxed. Claude has no browser on macOS and
//!   Linux for launch (`clauseo_profile::servers::no_browser`): agent-browser would have to run
//!   outside the sandbox, and Claude Code's exclusion check strips `env` and reworks `VAR=`
//!   prefixes before it matches, so `PATH=$CLAUDE_CODE_TMPDIR agent-browser …` would run a
//!   program Claude wrote unsandboxed, and quoting (`-q "upload"`, `e"v"al`, a tab) would run
//!   unsandboxed past the ask rules. Clauseo's own shim, named by its absolute path, is the
//!   after-launch way back (decision record 0030).
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
//!   commands' `TMPDIR` is made). Under a linked data folder every one of them, and every
//!   path in `allowRead`, is the real spelling: the launch prep re-spells the run
//!   (`launch_checks::folders::respell_run`), since Seatbelt has to read a link to walk it and
//!   the link sits under the denied home (stage C's Mac row on 0fcfd522b).
//! - `filesystem.denyWrite`: the matter's `.clauseo` folder, which holds `matter.json`, the
//!   marker that finds a moved matter folder again.
//! - `filesystem.denyRead`: the lawyer's home (other clients' files, `~/.ssh`), each spelling of
//!   `<data>` (it may lie outside the home), every other matter's folder the host knows at the
//!   launch, as stored and where it leads unless that is a place the sandbox must leave open
//!   ([`links`]: `/`, a folder above the session's own, a system folder; the launch then tells
//!   the lawyer), `/Volumes` and `/Users/Shared`; the data fence's `Read(…)` deny rules become read
//!   denies too. A matter registered after the launch is in no list: the accepted residual.
//! - `filesystem.allowRead` ([`ClaudeSandbox::allow_read`]): re-opens, within those denies,
//!   what a sandboxed command needs, with the `PATH` file Claude Code reads into each command
//!   (`<run>/clauseo-env.sh`, `agent::shell_path`), the trust roots file on macOS
//!   (`<data>/tls/roots.pem`, [`super::tls_roots`], the file alone) and each folder first on
//!   `PATH`. Not the whole run folder: it holds the case-law relay's token and a browser's
//!   profile (the visited sites' cookies). Of Claude's own project
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

use clauseo_protocol::ErrorCode;
use serde_json::{Map, Value, json};

use crate::error::Error;

pub(crate) mod glob;
pub(crate) mod links;

pub(crate) use links::OtherMatter;

/// The hosts a sandboxed command may not reach even through Claude Code's proxy: loopback, where
/// the browser's debugging port and agent-browser's daemon listen, and the any-address
/// `0.0.0.0` and `[::]`, which reach loopback too (stage C on a Mac, 2026-11-11). `127.1`, the
/// decimal forms and `[::ffff:127.0.0.1]` come out as `127.0.0.1` once the proxy reads them.
pub(crate) const DENIED_DOMAINS: [&str; 6] =
    ["localhost", "127.0.0.1", "::1", "[::1]", "0.0.0.0", "[::]"];

/// What sandboxed commands may not read beside `<data>` and the other matters: the home, as
/// Claude Code expands it, and macOS's shared places where matters may be kept.
pub(crate) const DENY_READ: [&str; 3] = ["~/", "/Volumes", "/Users/Shared"];

/// The characters Claude Code reads as a pattern in a sandbox path that no escape makes literal:
/// a path of the session's own holding one turns the sandbox off and refuses the launch
/// ([`Off::Wildcard`]); another matter's stays a pattern ([`glob`]). The UI names the folder by
/// the same set (`ui/app/src/room/folder-wildcard.ts`); change both.
pub(crate) const WILDCARDS: [char; 2] = ['*', '?'];

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
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Off {
    /// Windows (see the module doc).
    Windows,
    /// Claude's short temp folder was not made.
    NoTemp,
    /// A path to deny is not UTF-8: where another matter's folder leads, or `<data>`'s.
    NotUtf8,
    /// A path of the session's own, or of `<data>`, holds one of [`WILDCARDS`].
    Wildcard(Wild),
    /// Another matter's deny, its registered folder holding one of [`WILDCARDS`], would also
    /// deny one of the session's own folders: as the pattern on macOS, as the folder it is
    /// widened to on Linux ([`glob::fence_others`]).
    CoversOwn,
}

/// Where a wildcard sits: the deepest folder name holding one, in the matter folder's own path
/// when that holds one, else in the first path of the lists that does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Wild {
    /// The folder's name, as the refusal's words name it (the UI finds it the same way).
    pub(crate) folder: String,
    /// Whether it is in the matter folder's own path, which the lawyer can rename.
    pub(crate) in_matter: bool,
}

impl Off {
    /// The launch refusal for this reason: none on Windows, where no sandbox was meant to be;
    /// `matter.folder_wildcard` for a wildcard in the matter folder's path,
    /// `agent.sandbox_wildcard` for one in Clauseo's own folders or for another matter's pattern
    /// that would cover this session's folders ([`Off::CoversOwn`]), `matter.name_unreadable`
    /// for a name that is not UTF-8 (renaming fixes it, a restart never), and
    /// `agent.sandbox_unavailable` otherwise. The detail names no path.
    pub(crate) fn refusal(&self) -> Option<Error> {
        let (code, detail) = match self {
            Self::Windows => return None,
            Self::Wildcard(Wild {
                in_matter: true, ..
            }) => (
                ErrorCode::MatterFolderWildcard,
                "the matter folder's path holds * or ?, which Claude's sandbox cannot name",
            ),
            Self::Wildcard(_) => (
                ErrorCode::AgentSandboxWildcard,
                "a sandbox path outside the matter folder holds * or ?, which it cannot name",
            ),
            Self::CoversOwn => (
                ErrorCode::AgentSandboxWildcard,
                "another matter's folder holds * or ?, and its deny would cover this matter's",
            ),
            Self::NoTemp => (
                ErrorCode::AgentSandboxUnavailable,
                "Claude's short temp folder was not made, so it has no sandbox",
            ),
            Self::NotUtf8 => (
                ErrorCode::MatterNameUnreadable,
                "a folder Claude's sandbox must deny has a name that is not UTF-8",
            ),
        };
        Some(Error::refused(code, detail))
    }
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
    /// The names of the other matters where their folders lead was left out of the denies
    /// ([`links`]), for the launch's notice.
    pub(crate) links_left_out: Vec<String>,
}

/// The session's paths the sandbox names.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SandboxPaths<'a> {
    /// The matter folder (the agent's working folder).
    pub(crate) matter: &'a Path,
    /// Its other spellings (the launch prep's `folders::own_spellings`): as stored, where it
    /// leads, as on disk, the firmlink twins.
    pub(crate) matter_also: &'a [PathBuf],
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
    /// stetdoc's (the shell browser's tools never: Claude has no browser where it has a
    /// sandbox), then the profile hook's (Clauseo's Python runtime, `<rt>/bin`, where pandoc
    /// is too).
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
    /// Every other matter the host knows, its folder as stored and where it leads.
    pub(crate) other_matters: &'a [OtherMatter],
}

/// The prefix of the Python runtime's folder name (`agents::python`).
const RUNTIME_PREFIX: &str = "clauseo-python-";

impl ClaudeSandbox {
    /// A Claude launch's sandbox for `paths` on `os`: none on Windows, else the sandbox or the
    /// launch's refusal ([`ClaudeSandbox::for_os`], [`Off::refusal`]), never neither.
    pub(crate) fn for_launch(
        os: SandboxOs,
        paths: &SandboxPaths<'_>,
    ) -> Result<Option<Self>, Error> {
        match Self::for_os(os, paths) {
            Ok(sandbox) => Ok(Some(sandbox)),
            Err(off) => off.refusal().map_or(Ok(None), Err),
        }
    }

    /// The sandbox for `paths` on `os`; none on Windows, without Claude's short temp folder,
    /// when a path to deny is not UTF-8, when a path of its own or `<data>` holds one of
    /// [`WILDCARDS`], or when another matter's deny holding one would cover its own folders (one
    /// warn line each, naming only a wildcard's folder). A `[` is escaped ([`escape_brackets`]).
    pub(crate) fn for_os(os: SandboxOs, paths: &SandboxPaths<'_>) -> Result<Self, Off> {
        if os == SandboxOs::Windows {
            return Err(Off::Windows);
        }
        let Some(claude_tmp) = paths.claude_tmp else {
            tracing::warn!("Claude's sandbox is off: its temp folder was not made; refused");
            return Err(Off::NoTemp);
        };
        let marker = paths.matter.join(MATTER_MARKER);
        let own = [paths.matter, paths.work, paths.tmp, paths.notes, claude_tmp];
        let mut sandbox = Self {
            allow_write: texts(own),
            deny_write: texts([marker.as_path()]),
            deny_read: Vec::new(),
            allow_read: Self::allow_read(paths, claude_tmp),
            quiet: texts([paths.claude_home.join(SNAPSHOTS).as_path()]),
            links_left_out: Vec::new(),
        };
        let denies = sandbox.other_denies(os, paths);
        let denied = paths.data.iter().chain(&denies.spellings);
        let Some(mut deny_read) = strict_texts(denied.map(PathBuf::as_path)) else {
            not_utf8(paths, &denies.spellings);
            return Err(Off::NotUtf8);
        };
        deny_read.splice(0..0, DENY_READ.iter().map(|path| (*path).to_owned()));
        sandbox.deny_read = deny_read;
        sandbox.links_left_out = denies.left_out;
        let others = texts(denies.spellings.iter().map(PathBuf::as_path));
        if let Some(wild) = sandbox.wildcard(paths.matter, &others) {
            tracing::warn!(
                folder = %wild.folder,
                in_matter = wild.in_matter,
                "Claude's sandbox is off: a sandbox path holds a wildcard; refused"
            );
            return Err(Off::Wildcard(wild));
        }
        let own_matter =
            std::iter::once(paths.matter).chain(paths.matter_also.iter().map(PathBuf::as_path));
        let own_matter = texts(own_matter);
        if glob::fence_others(os, &mut sandbox, &own_matter).is_err() {
            tracing::warn!(
                "Claude's sandbox is off: another matter's deny covers its own; refused"
            );
            return Err(Off::CoversOwn);
        }
        let mut sandbox = sandbox.escaped(os);
        sandbox
            .allow_read
            .extend(tool_results(paths.claude_home, paths.matter));
        Ok(sandbox)
    }

    /// The other matters' read-deny spellings ([`links::other_denies`]), against what the
    /// session must keep open: its matter (each spelling), Claude's config folder and every
    /// folder its lists open.
    fn other_denies(&self, os: SandboxOs, paths: &SandboxPaths<'_>) -> links::OtherDenies {
        let lists = [&self.allow_write, &self.allow_read, &self.quiet];
        let mut open: Vec<&Path> = lists.into_iter().flatten().map(Path::new).collect();
        open.extend([paths.matter, paths.claude_home]);
        open.extend(paths.matter_also.iter().map(PathBuf::as_path));
        links::other_denies(os, paths.other_matters, &open)
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
            links_left_out: self.links_left_out,
        }
    }

    /// Where a path in the lists holds one of [`WILDCARDS`], if one does: in `matter`'s own
    /// path first ([`Wild`]). Not another matter's deny (`others`), which
    /// [`glob::fence_others`] keeps as a pattern or widens.
    fn wildcard(&self, matter: &Path, others: &[String]) -> Option<Wild> {
        let own = [&self.allow_write, &self.allow_read, &self.quiet];
        let denied = [&self.deny_write, &self.deny_read];
        let wild = |path: &&String| path.contains(WILDCARDS);
        let first = own.into_iter().flatten().find(wild).or_else(|| {
            let mut denied = denied.into_iter().flatten().filter(wild);
            denied.find(|path| !others.contains(path))
        })?;
        let matter = matter.to_str().filter(|text| text.contains(WILDCARDS));
        let in_matter = matter.is_some();
        let folder = wild_folder(matter.unwrap_or(first)).to_owned();
        Some(Wild { folder, in_matter })
    }

    /// `filesystem.allowRead`: the writable folders, the browser's downloads, the skill packs,
    /// the built-in skills, the shells' `PATH` file, the trust roots file, and each folder first
    /// on `PATH` (stetdoc's), except that a folder in Clauseo's
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
            "failIfUnavailable": true,
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
        if let Some(object) = sandbox.as_object_mut()
            && !self.quiet.is_empty()
        {
            object.insert("ignoreViolations".to_owned(), json!({"*": self.quiet}));
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

/// The deepest folder name in `path` that holds one of [`WILDCARDS`] (the whole path if none).
pub(crate) fn wild_folder(path: &str) -> &str {
    path.rsplit('/')
        .find(|name| name.contains(WILDCARDS))
        .unwrap_or(path)
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

/// One warn line for a deny that is not UTF-8: the other matter's id whose folder holds or
/// leads to it, else `<data>`'s; never the path.
fn not_utf8(paths: &SandboxPaths<'_>, spellings: &[PathBuf]) {
    let bad = |path: &PathBuf| path.to_str().is_none();
    let matter = paths.other_matters.iter().find(|other| {
        let denied = |also: &&PathBuf| spellings.contains(also) && bad(also);
        bad(&other.stored) || other.also.iter().any(|also| denied(&also))
    });
    match matter {
        Some(other) => tracing::warn!(
            matter = ?other.id,
            "Claude's sandbox is off: another matter's folder leads to a name that is not UTF-8; \
             refused"
        ),
        None => tracing::warn!(
            "Claude's sandbox is off: the data folder's real path has a name that is not UTF-8; \
             refused"
        ),
    }
}

/// [`texts`], or none when a path is not UTF-8.
fn strict_texts<'a>(paths: impl IntoIterator<Item = &'a Path>) -> Option<Vec<String>> {
    let paths: Vec<&Path> = paths.into_iter().collect();
    paths
        .iter()
        .all(|path| path.to_str().is_some())
        .then(|| texts(paths))
}
