//! Building one report from what is known at the crash: the envelope from [`About`], the week
//! from the clock, and the kind's own field. No file is read and nothing is locked, so the
//! panic hook can call it.

use std::io;
use std::path::{Path, PathBuf};

use crate::about::About;
use crate::schema::{
    CrashArea, CrashKind, CrashProcess, CrashReport, CrashReportId, Line, PanicLocation,
    RendererGoneReason, SchemaVersion, SignalCode, SourceFile, Time, crash_body,
};
use crate::write;

/// A report made and not yet written: its bytes and the file they go to.
#[derive(Debug)]
pub struct Pending {
    path: PathBuf,
    body: Vec<u8>,
}

impl Pending {
    /// `report`'s checked body, to go in `dir` as `<secs>-<id>.json`.
    pub(crate) fn new(dir: &Path, report: &CrashReport, now: Time) -> io::Result<Pending> {
        let week = now.day().ok_or(io::ErrorKind::InvalidInput)?.week();
        let body = crash_body(report, week)
            .map_err(|invalid| io::Error::other(format!("a crash report refused: {invalid:?}")))?;
        let name = format!("{}-{}.json", now.unix_secs(), report.report);
        Ok(Pending {
            path: dir.join(name),
            body,
        })
    }

    /// Writes the report whole (a temp file, then a rename), so a reader never sees half.
    pub fn write(self) -> io::Result<()> {
        write::replace(&self.path, &self.body)
    }
}

/// The report's envelope at `now`, for a crash of `kind` in `process`.
fn envelope(
    about: &About,
    now: Time,
    process: CrashProcess,
    kind: CrashKind,
    area: CrashArea,
    after_launch: bool,
) -> Option<CrashReport> {
    Some(CrashReport {
        schema: SchemaVersion::V1,
        report: CrashReportId::random(),
        week: now.day()?.week(),
        version: about.version,
        channel: about.channel,
        os: about.platform.os(),
        os_major: about.platform.os_major(),
        arch: about.arch,
        process,
        kind,
        area,
        after_launch,
        location: None,
        code: None,
        reason: None,
    })
}

/// A panic's report: its location made relative ([`source_file`]), its area from the file's
/// crate, else the thread's name ([`area_of_thread`]).
pub(crate) fn panic(
    about: &About,
    after_launch: bool,
    file: Option<&str>,
    line: u32,
    thread: Option<&str>,
    now: Time,
) -> Option<CrashReport> {
    let file = file.and_then(source_file);
    let area = file
        .as_ref()
        .and_then(SourceFile::area)
        .unwrap_or_else(|| area_of_thread(thread));
    let location = file
        .zip(Line::new(line))
        .map(|(file, line)| PanicLocation { file, line });
    let mut report = envelope(
        about,
        now,
        CrashProcess::Host,
        CrashKind::Panic,
        area,
        after_launch,
    )?;
    report.location = location;
    Some(report)
}

/// A native crash's report, from the code its handler wrote.
pub(crate) fn signal(
    about: &About,
    after_launch: bool,
    code: SignalCode,
    now: Time,
) -> Option<CrashReport> {
    let mut report = envelope(
        about,
        now,
        CrashProcess::Host,
        CrashKind::Signal,
        CrashArea::Other,
        after_launch,
    )?;
    report.code = Some(code);
    Some(report)
}

/// The webview's renderer gone.
pub(crate) fn renderer_gone(
    about: &About,
    after_launch: bool,
    reason: RendererGoneReason,
    now: Time,
) -> Option<CrashReport> {
    let mut report = envelope(
        about,
        now,
        CrashProcess::Webview,
        CrashKind::RendererGone,
        CrashArea::Webview,
        after_launch,
    )?;
    report.reason = Some(reason);
    Some(report)
}

/// A panic's file as the build recorded it, made relative: a workspace crate's
/// `crates/<crate>/src/...rs` (whatever folder the build ran in), or a registry dependency's
/// `<dep>-<version>/src/...rs` (the registry's folder dropped). `None` for any other path (std,
/// a git dependency, generated code), so no folder of the build machine travels.
pub fn source_file(raw: &str) -> Option<SourceFile> {
    let path = raw.replace('\\', "/");
    if let Ok(file) = path.parse() {
        return Some(file);
    }
    let in_workspace = path
        .match_indices("/crates/")
        .find_map(|(at, _)| path[at + 1..].parse().ok());
    if in_workspace.is_some() {
        return in_workspace;
    }
    let (_, registry) = path.split_once("/registry/src/")?;
    let (_index, dep) = registry.split_once('/')?;
    dep.parse().ok()
}

/// The area of a crashing thread, by the names Clauseo gives its threads: `main` is desktop's,
/// `clauseo-host*` and `clauseo-log*` the host's, `clauseo-store*` the store's, `clauseo-relay*`
/// the connectors', `clauseo-setup*` desktop's. Any other name, or none, is `other`.
pub fn area_of_thread(name: Option<&str>) -> CrashArea {
    let Some(name) = name else {
        return CrashArea::Other;
    };
    if name == "main" {
        return CrashArea::Desktop;
    }
    let prefixes = [
        ("clauseo-host", CrashArea::Host),
        ("clauseo-log", CrashArea::Host),
        ("clauseo-store", CrashArea::Store),
        ("clauseo-relay", CrashArea::Connectors),
        ("clauseo-setup", CrashArea::Desktop),
    ];
    prefixes
        .iter()
        .find(|(prefix, _)| name.starts_with(prefix))
        .map_or(CrashArea::Other, |&(_, area)| area)
}
