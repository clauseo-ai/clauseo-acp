//! The crash report of S22's launch cut (decision 0060): one crash, as a few closed fields.
//! `POST /v1/crash` takes one [`CrashReport`]; the Worker reads it through [`parse_crash`] and
//! the app writes it through [`crash_body`], so both keep the same rules.
//!
//! A report holds no install id, no tally id, no time finer than the ISO week, no message and no
//! path but a source file of the two forms [`SOURCE_FILE_PATTERN`] allows. Every string is an
//! enum or one fixed form, every integer is capped.

use std::fmt;
use std::str::FromStr;

use super::closed::{closed_enum, seal};
use super::text::as_text;
use super::{
    Arch, BadText, Channel, Crash, CrashKind, CrashProcess, CrashReportId, Invalid, Line, Os,
    OsMajor, Platform, SchemaVersion, SignalCode, Version, Week,
};

/// The largest gzip body for `/v1/crash`, and the largest JSON it may inflate to. The largest
/// report the types allow is under 1 KB (`schema/tests/crash.rs`), far under the 128 KB a
/// Worker request may be.
pub const MAX_CRASH_GZIP: usize = 2_000;
/// See [`MAX_CRASH_GZIP`].
pub const MAX_CRASH_JSON: usize = 2_000;
/// How old a report's week may be when it is sent: a report is kept 30 days, which spans at
/// most 5 ISO weeks after the one it was found in.
pub const MAX_CRASH_AGE_WEEKS: i64 = 5;
/// The longest source file.
pub const SOURCE_FILE_MAX: usize = 200;
/// The highest signal number on macOS and Linux.
pub const MAX_UNIX_SIGNAL: u32 = 64;

/// The pattern of a panic's source file on the wire, at most [`SOURCE_FILE_MAX`] long: one of
/// Clauseo's crates, `crates/<crate>/src/...rs`, or a dependency from the registry,
/// `<dep>-<version>/src/...rs`. Folders and the file are letters, digits and `_`, at most 10
/// folders deep. [`SourceFile`]'s parser is this pattern, and a test holds the crate names to
/// [`CrashArea`]'s.
pub const SOURCE_FILE_PATTERN: &str = concat!(
    "^(crates/(acp|agents|blocklist|codex_appserver|connectors|counts|desktop|host|limits|",
    "matters|mcp|process|profile|protocol|reducer|sandbox|search|settings|store)",
    "|[A-Za-z][A-Za-z0-9_-]{0,63}-(0|[1-9][0-9]{0,8})\\.(0|[1-9][0-9]{0,8})\\.(0|[1-9][0-9]{0,8})",
    "(-[0-9A-Za-z]{1,16}(\\.[0-9A-Za-z]{1,16}){0,3})?)",
    "/src/([A-Za-z0-9_]{1,64}/){0,10}[A-Za-z0-9_]{1,64}\\.rs$",
);

closed_enum! {
    "Where Clauseo stopped: the crate the panic came from (its source file's, else the"
    "crashing thread's), `webview` for the renderer, `other` when neither says."
    pub enum CrashArea {
        "`crates/acp`."
        Acp = "acp",
        "`crates/agents`."
        Agents = "agents",
        "`crates/blocklist`."
        Blocklist = "blocklist",
        "`crates/codex_appserver`."
        CodexAppserver = "codex_appserver",
        "`crates/connectors`."
        Connectors = "connectors",
        "`crates/counts`."
        Counts = "counts",
        "`crates/desktop`."
        Desktop = "desktop",
        "`crates/host`."
        Host = "host",
        "`crates/limits`."
        Limits = "limits",
        "`crates/matters`."
        Matters = "matters",
        "`crates/mcp`."
        Mcp = "mcp",
        "`crates/process`."
        Process = "process",
        "`crates/profile`."
        Profile = "profile",
        "`crates/protocol`."
        Protocol = "protocol",
        "`crates/reducer`."
        Reducer = "reducer",
        "`crates/sandbox`."
        Sandbox = "sandbox",
        "`crates/search`."
        Search = "search",
        "`crates/settings`."
        Settings = "settings",
        "`crates/store`."
        Store = "store",
        "The webview's renderer."
        Webview = "webview",
        "A dependency, the OS, or no crate known."
        Other = "other",
    }
}

impl CrashArea {
    /// The area of the crate in `crates/<dir>`, or `None` when `dir` is not one of Clauseo's.
    pub fn from_crate_dir(dir: &str) -> Option<CrashArea> {
        Self::ALL
            .iter()
            .copied()
            .filter(|area| !matches!(area, CrashArea::Webview | CrashArea::Other))
            .find(|area| area.as_str() == dir)
    }
}

closed_enum! {
    "Why the webview's renderer went, as the OS says. Windows' `ProcessFailed` gives the"
    "reason; Linux's `web-process-terminated` gives crashed, out of memory or terminated;"
    "macOS gives none, so its reports say `other`."
    pub enum RendererGoneReason {
        "It crashed."
        Crashed = "crashed",
        "It ran out of memory, or past the OS's limit."
        OutOfMemory = "out_of_memory",
        "It stopped answering and was ended."
        Unresponsive = "unresponsive",
        "Something outside it ended it."
        Terminated = "terminated",
        "It failed to start."
        LaunchFailed = "launch_failed",
        "No reason given, or one not listed."
        Other = "other",
    }
}

/// A panic's source file in one of [`SOURCE_FILE_PATTERN`]'s two forms, read only in that form.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SourceFile(String);

impl SourceFile {
    /// The path, as it travels.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The Clauseo crate the file is in, or `None` for a dependency's.
    pub fn area(&self) -> Option<CrashArea> {
        let rest = self.0.strip_prefix("crates/")?;
        let (dir, _) = rest.split_once('/')?;
        CrashArea::from_crate_dir(dir)
    }
}

impl fmt::Display for SourceFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for SourceFile {
    type Err = BadText;

    /// Exactly [`SOURCE_FILE_PATTERN`], at most [`SOURCE_FILE_MAX`] long.
    fn from_str(text: &str) -> Result<SourceFile, BadText> {
        is_source_file(text)
            .then(|| SourceFile(text.to_owned()))
            .ok_or(BadText)
    }
}

as_text!(
    SourceFile,
    "SourceFile",
    schema: schemars::json_schema!({
        "type": "string",
        "maxLength": SOURCE_FILE_MAX,
        "pattern": SOURCE_FILE_PATTERN,
    })
);

/// Whether `text` fits [`SOURCE_FILE_PATTERN`] and [`SOURCE_FILE_MAX`].
fn is_source_file(text: &str) -> bool {
    if text.len() > SOURCE_FILE_MAX {
        return false;
    }
    let Some((root, rest)) = text.split_once("/src/") else {
        return false;
    };
    let root_fits = match root.strip_prefix("crates/") {
        Some(dir) => CrashArea::from_crate_dir(dir).is_some(),
        None => is_dep_root(root),
    };
    root_fits && is_src_path(rest)
}

/// `([A-Za-z0-9_]{1,64}/){0,10}[A-Za-z0-9_]{1,64}\.rs`.
fn is_src_path(rest: &str) -> bool {
    let Some(path) = rest.strip_suffix(".rs") else {
        return false;
    };
    let parts: Vec<&str> = path.split('/').collect();
    (1..=11).contains(&parts.len()) && parts.iter().all(|part| is_segment(part))
}

/// `[A-Za-z0-9_]{1,64}`.
fn is_segment(part: &str) -> bool {
    (1..=64).contains(&part.len()) && part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// `<dep>-<version>`: some `-` splits it into a name and a version that each fit.
fn is_dep_root(root: &str) -> bool {
    root.char_indices()
        .filter(|&(_, c)| c == '-')
        .any(|(at, _)| is_dep_name(&root[..at]) && is_dep_version(&root[at + 1..]))
}

/// `[A-Za-z][A-Za-z0-9_-]{0,63}`.
fn is_dep_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    let first = bytes.next().is_some_and(|b| b.is_ascii_alphabetic());
    first && name.len() <= 64 && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// `N.N.N` with an optional `-ident(.ident){0,3}`, each `N` `0|[1-9][0-9]{0,8}` and each ident
/// `[0-9A-Za-z]{1,16}`.
fn is_dep_version(version: &str) -> bool {
    let (core, pre) = match version.split_once('-') {
        Some((core, pre)) => (core, Some(pre)),
        None => (version, None),
    };
    let numbers: Vec<&str> = core.split('.').collect();
    let core_fits = numbers.len() == 3
        && numbers.iter().all(|n| {
            (1..=9).contains(&n.len())
                && n.bytes().all(|b| b.is_ascii_digit())
                && (*n == "0" || !n.starts_with('0'))
        });
    let pre_fits = pre.is_none_or(|pre| {
        let idents: Vec<&str> = pre.split('.').collect();
        (1..=4).contains(&idents.len())
            && idents.iter().all(|ident| {
                (1..=16).contains(&ident.len()) && ident.bytes().all(|b| b.is_ascii_alphanumeric())
            })
    });
    core_fits && pre_fits
}

/// Where a panic happened: its source file and line, never its message.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
pub struct PanicLocation {
    /// Relative to the workspace, or to the registry for a dependency.
    pub file: SourceFile,
    /// The line.
    pub line: Line,
}

seal!(PanicLocation { file, line });

/// One crash, found on the lawyer's machine and sent with counts on. No install id, so it can
/// never be joined to one; the week, not a time.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
pub struct CrashReport {
    /// `1`.
    pub schema: SchemaVersion,
    /// This report's own random id.
    pub report: CrashReportId,
    /// The ISO week the crash was found in.
    pub week: Week,
    /// The app's version.
    pub version: Version,
    /// `stable` or `beta`.
    pub channel: Channel,
    /// The OS.
    pub os: Os,
    /// The OS's major version; Linux leaves it out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os_major: Option<OsMajor>,
    /// The CPU architecture.
    pub arch: Arch,
    /// `host` for a panic or a signal, `webview` for a renderer gone.
    pub process: CrashProcess,
    /// What kind: `panic`, `signal` or `renderer_gone`.
    pub kind: CrashKind,
    /// Where Clauseo stopped.
    pub area: CrashArea,
    /// Whether the window had shown the app before the crash.
    pub after_launch: bool,
    /// `panic` only: its file and line, left out when the file fits neither form.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<PanicLocation>,
    /// `signal` only, and always there: the signal number or exception code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<SignalCode>,
    /// `renderer_gone` only, and always there: why the renderer went.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<RendererGoneReason>,
}

seal!(CrashReport {
    schema,
    report,
    week,
    version,
    channel,
    os,
    os_major,
    arch,
    process,
    kind,
    area,
    after_launch,
    location,
    code,
    reason
});

impl CrashReport {
    /// The schema's rules beyond its types, for a report sent in the week `this_week`: its week
    /// is this one or at most 5 before, its OS and major version pair, and its kind has its own
    /// fields and no other's. `stall` waits for full S22, so it is refused.
    pub fn check(&self, this_week: Week) -> Result<(), Invalid> {
        let age = this_week.weeks_since(self.week);
        if !(0..=MAX_CRASH_AGE_WEEKS).contains(&age) {
            return Err(Invalid::OutOfWindow);
        }
        Platform::from_parts(self.os, self.os_major).ok_or(Invalid::Platform)?;
        let host = self.process == CrashProcess::Host;
        let fits = match self.kind {
            CrashKind::Panic => host && self.code.is_none() && self.reason.is_none(),
            CrashKind::Signal => {
                let unix_fits = self.os == Os::Windows
                    || self.code.is_some_and(|code| code.get() <= MAX_UNIX_SIGNAL);
                host && self.code.is_some()
                    && unix_fits
                    && self.location.is_none()
                    && self.reason.is_none()
            }
            CrashKind::RendererGone => {
                self.process == CrashProcess::Webview
                    && self.area == CrashArea::Webview
                    && self.reason.is_some()
                    && self.location.is_none()
                    && self.code.is_none()
            }
            CrashKind::Stall => false,
        };
        fits.then_some(()).ok_or(Invalid::Detail)
    }

    /// The weekly tally's `crash` event for this crash.
    pub const fn tally_event(&self) -> Crash {
        Crash {
            process: self.process,
            kind: self.kind,
            after_launch: self.after_launch,
        }
    }
}

/// Reads an inflated `/v1/crash` body: at most 2 KB of JSON, the strict types, then
/// [`CrashReport::check`].
pub fn parse_crash(json: &[u8], this_week: Week) -> Result<CrashReport, Invalid> {
    if json.len() > MAX_CRASH_JSON {
        return Err(Invalid::TooLarge);
    }
    let report: CrashReport = serde_json::from_slice(json).map_err(Invalid::from)?;
    report.check(this_week)?;
    Ok(report)
}

/// The JSON body of `report`, made in the week `this_week`: [`CrashReport::check`], then the
/// bytes, refused past [`MAX_CRASH_JSON`]. These are the bytes stored and sent, before gzip,
/// and [`parse_crash`] reads them back to the same report.
pub fn crash_body(report: &CrashReport, this_week: Week) -> Result<Vec<u8>, Invalid> {
    report.check(this_week)?;
    let json = serde_json::to_vec(report).map_err(Invalid::from)?;
    if json.len() > MAX_CRASH_JSON {
        return Err(Invalid::TooLarge);
    }
    Ok(json)
}

#[cfg(test)]
impl super::tests::sample::Sample for SourceFile {
    fn sample(tape: &mut super::tests::sample::Tape) -> Self {
        const SEGMENTS: &[&str] = &["schema", "log_file", "Win32", "os", "a1_b2", "mod"];
        let root = if tape.below(2) == 0 {
            let crates: Vec<CrashArea> = CrashArea::ALL
                .iter()
                .copied()
                .filter(|area| area.as_str() != "webview" && area.as_str() != "other")
                .collect();
            format!("crates/{}", tape.pick(&crates).as_str())
        } else {
            const DEPS: &[&str] = &[
                "tokio-1.54.1",
                "windows-sys-0.59.0",
                "h2-0.4.6",
                "x-0.1.0-rc.1",
            ];
            tape.pick(DEPS).to_owned()
        };
        let folders: Vec<&str> = (0..tape.below(4)).map(|_| tape.pick(SEGMENTS)).collect();
        let mut path = format!("{root}/src/");
        for folder in folders {
            path.push_str(folder);
            path.push('/');
        }
        path.push_str(tape.pick(SEGMENTS));
        path.push_str(".rs");
        path.parse().unwrap()
    }
}
