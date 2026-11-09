//! The negative check: a second package B must not reach package A's namespace or files.
//!
//! The owner builds B's namespace beside A's (`Namespace::build`) and copies its handles into
//! the helper as it does A's. B gets no file grants and no loopback exemption; its only ACEs
//! are on the private station and desktop, which any process that loads user32 needs. Before
//! chrome starts, the helper runs the probe: the test binary again ([`PROBE_EXE`], the mark
//! `lowbox_probe` looks for), started with `LowBoxCommand::spawn_suspended` over B's handles,
//! so in B's `LowBox` token. It opens, with the same access for each side:
//! - A's package directory (`NtOpenDirectoryObject`, query and traverse) and A's pipe folder
//!   (`NtCreateFile`, `FILE_OPEN`, `READ_CONTROL | SYNCHRONIZE`): each must be refused with
//!   `STATUS_ACCESS_DENIED`;
//! - each of A's four granted folders (profile, staging, tmp, shell): a listing and a read of
//!   the owner's [`MARKER`] file in it, each refused with `ERROR_ACCESS_DENIED`;
//! - the positive controls: B's own directory and pipe folder, and the probe's folder (which
//!   ALL APPLICATION PACKAGES may read, so both packages can), listed and its marker read. Each
//!   must open, so a probe that cannot open anything cannot pass.
//!
//! After chrome's processes have started and the helper has inspected them, the helper runs the
//! probe a second time in B's token, now with one target per chrome pid it holds a handle for
//! (its handle keeps the process object, so the pid cannot be reused). The probe calls
//! `OpenProcess` on each pid once per right in [`PROCESS_RIGHTS`]
//! (`PROCESS_QUERY_LIMITED_INFORMATION`, `PROCESS_VM_READ`, `PROCESS_TERMINATE`,
//! `PROCESS_DUP_HANDLE`); every one must be refused with `ERROR_ACCESS_DENIED`.
//!
//! One `probe target=<label> api=<nt|win32> code=<hex>` line per open; the owner judges them
//! (`evidence.rs`); the helper's `probe-exit phase=<files|procs>` line follows each run. The paths
//! and pids travel in the child's environment ([`TARGETS_VAR`]), the only
//! variables it has besides the system folders.
#![expect(
    unsafe_code,
    reason = "FFI to NtOpenDirectoryObject, NtCreateFile, OpenProcess, the Job Object and the wait"
)]

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::os::windows::io::OwnedHandle;
use std::path::Path;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

use clauseo_lowbox::{LowBoxCommand, NamespaceHandles, Stdio};
use windows::Wdk::Foundation::OBJECT_ATTRIBUTES;
use windows::Wdk::Storage::FileSystem::{
    FILE_DIRECTORY_FILE, FILE_OPEN, NtCreateFile, NtOpenDirectoryObject,
};
use windows::Win32::Foundation::{HANDLE, OBJ_CASE_INSENSITIVE, UNICODE_STRING, WAIT_OBJECT_0};
use windows::Win32::Storage::FileSystem::{
    FILE_ACCESS_RIGHTS, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE,
};
use windows::Win32::System::IO::IO_STATUS_BLOCK;
use windows::Win32::System::JobObjects::AssignProcessToJobObject;
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_ACCESS_RIGHTS, PROCESS_DUP_HANDLE, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_TERMINATE, PROCESS_VM_READ, WaitForSingleObject,
};
use windows::core::PWSTR;

use super::helper::{MARK, kill_on_close_job, say, system_var};
use super::inspect;
use super::win::{Res, code, failed, owned, raw};

/// The probe's file name in its folder: the mark that makes `lowbox_probe` do the probe's work.
pub(crate) const PROBE_EXE: &str = "clauseo-lowbox-proof-probe.exe";
/// The file the owner writes in each of A's folders and in the probe's folder, for the reads.
pub(crate) const MARKER: &str = "clauseo-probe-marker.txt";
/// The marker's text.
pub(crate) const MARKER_TEXT: &str = "clauseo lowbox probe marker\n";
/// The child's variable that lists the targets.
const TARGETS_VAR: &str = "CLAUSEO_PROBE_TARGETS";
/// The longest the helper waits for the probe to exit.
pub(crate) const PROBE_EXIT_MS: u32 = 30_000;
/// The longest it waits for the probe's output to close after the exit.
const OUTPUT_WAIT: Duration = Duration::from_secs(10);
/// `DIRECTORY_QUERY | DIRECTORY_TRAVERSE`: the directory opens' access, on both packages.
const DIRECTORY_PROBE: u32 = 0x0000_0003;
/// `READ_CONTROL | SYNCHRONIZE`: the pipe-folder opens' access, which B's own folder grants B.
const PIPES_PROBE: u32 = 0x0012_0000;
/// The rights the probe asks `OpenProcess` for on each of A's chrome processes, one call each,
/// with the suffix of its line's label.
pub(crate) const PROCESS_RIGHTS: [(&str, PROCESS_ACCESS_RIGHTS); 4] = [
    ("query", PROCESS_QUERY_LIMITED_INFORMATION),
    ("vm-read", PROCESS_VM_READ),
    ("terminate", PROCESS_TERMINATE),
    ("dup-handle", PROCESS_DUP_HANDLE),
];

/// What a target is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// An object directory, opened with `NtOpenDirectoryObject`.
    Directory,
    /// A pipe folder, opened with `NtCreateFile`.
    Pipes,
    /// A disk folder, listed, and its [`MARKER`] read.
    Folder,
    /// A process id, opened with each of [`PROCESS_RIGHTS`].
    Process,
}

impl Kind {
    /// Its name in [`TARGETS_VAR`].
    fn name(self) -> &'static str {
        match self {
            Kind::Directory => "dir",
            Kind::Pipes => "pipes",
            Kind::Folder => "folder",
            Kind::Process => "process",
        }
    }
}

/// One thing the probe opens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Target {
    /// Its label: `a-…` must be refused, `b-…` and `shared` must open.
    pub(crate) label: String,
    /// What it is.
    pub(crate) kind: Kind,
    /// Its kernel object name or disk path.
    pub(crate) path: String,
}

/// `targets` as [`TARGETS_VAR`]'s value: `label|kind|path` entries split by tabs (`|` is in no
/// disk path, nor in the row's object names).
fn encode(targets: &[Target]) -> String {
    targets
        .iter()
        .map(|t| format!("{}|{}|{}", t.label, t.kind.name(), t.path))
        .collect::<Vec<String>>()
        .join("\t")
}

/// The targets in `text`.
fn decode(text: &str) -> Res<Vec<Target>> {
    text.split('\t')
        .map(|entry| {
            let mut parts = entry.splitn(3, '|');
            let (Some(label), Some(kind), Some(path)) = (parts.next(), parts.next(), parts.next())
            else {
                return Err(format!("a probe target is not label|kind|path: {entry}"));
            };
            let kind = [Kind::Directory, Kind::Pipes, Kind::Folder, Kind::Process]
                .into_iter()
                .find(|k| k.name() == kind)
                .ok_or_else(|| format!("a probe target has the unknown kind {kind}"))?;
            Ok(Target {
                label: label.to_owned(),
                kind,
                path: path.to_owned(),
            })
        })
        .collect()
}

/// Whether this process is the probe (its argv[0] names [`PROBE_EXE`]).
pub(crate) fn is_probe() -> bool {
    std::env::args().next().is_some_and(|arg0| {
        Path::new(&arg0)
            .file_name()
            .is_some_and(|name| name.eq_ignore_ascii_case(PROBE_EXE))
    })
}

/// The helper's side: starts the probe (`exe`) in B's `LowBox` token over `namespace_b`, in a
/// Job Object of its own, waits for it and repeats its report lines, then a
/// `probe-exit phase=<phase>` line.
pub(crate) fn run_in_b(
    namespace_b: &NamespaceHandles,
    exe: &Path,
    targets: &[Target],
    phase: &str,
) -> Res<()> {
    let mut command = LowBoxCommand::new(exe);
    command
        .args([
            "--exact",
            "lowbox_probe",
            "--ignored",
            "--nocapture",
            "--test-threads",
            "1",
        ])
        .env("SystemRoot", system_var("SystemRoot", r"C:\Windows"))
        .env("windir", system_var("windir", r"C:\Windows"))
        .env("SystemDrive", system_var("SystemDrive", "C:"))
        .env(TARGETS_VAR, encode(targets))
        .current_dir(exe.parent().unwrap_or(exe))
        .stdin(Stdio::Null)
        .stdout(Stdio::Piped)
        .stderr(Stdio::Inherit);
    let job = kill_on_close_job()?;
    // A failure before the resume drops `child`, whose Drop ends the never-resumed process.
    let mut child = command
        .spawn_suspended(namespace_b)
        .map_err(|e| format!("spawn_suspended(package B): {e}"))?;
    // SAFETY: an open job and the child's open process handle (all access).
    unsafe { AssignProcessToJobObject(raw(&job), raw(&child.process)) }
        .map_err(|e| failed("AssignProcessToJobObject(probe)", &e))?;
    child.resume().map_err(|e| format!("resume(probe): {e}"))?;
    let output = child.stdout.take().map(read_lines);
    // SAFETY: the child's open process handle.
    let waited = unsafe { WaitForSingleObject(raw(&child.process), PROBE_EXIT_MS) };
    let code = inspect::exit_code(&child.process).map_or("-".to_owned(), |c| format!("{c:#x}"));
    let lines = output
        .and_then(|lines| lines.recv_timeout(OUTPUT_WAIT).ok())
        .unwrap_or_default();
    for line in &lines {
        if let Some(at) = line.find(MARK) {
            println!("{}", &line[at..]);
        }
    }
    say(format_args!(
        "probe-exit phase={phase} exited={} code={code} lines={}",
        u8::from(waited == WAIT_OBJECT_0),
        lines.len()
    ));
    drop(job);
    Ok(())
}

/// Reads `pipe` to its end on a thread of its own; the receiver gets every line.
fn read_lines(pipe: File) -> Receiver<Vec<String>> {
    let (done, lines) = mpsc::channel();
    thread::spawn(move || {
        let all: Vec<String> = BufReader::new(pipe).lines().map_while(Result::ok).collect();
        let _helper_gone = done.send(all);
    });
    lines
}

/// The probe's side, in B's token: opens each target and prints one line per open.
pub(crate) fn run() {
    let text = std::env::var(TARGETS_VAR)
        .unwrap_or_else(|_unset| panic!("the probe has no {TARGETS_VAR}"));
    let targets = decode(&text).unwrap_or_else(|e| panic!("{e}"));
    say(format_args!("probe-start targets={}", targets.len()));
    for target in &targets {
        match target.kind {
            Kind::Directory => nt_line(&target.label, open_directory(&target.path)),
            Kind::Pipes => nt_line(&target.label, open_pipes(&target.path)),
            Kind::Folder => {
                let folder = Path::new(&target.path);
                let listed = std::fs::read_dir(folder).map(|_entries| ());
                win32_line(&format!("{}-list", target.label), listed);
                let read = std::fs::read(folder.join(MARKER)).and_then(|bytes| {
                    if bytes == MARKER_TEXT.as_bytes() {
                        Ok(())
                    } else {
                        Err(std::io::Error::other("the marker's text differs"))
                    }
                });
                win32_line(&format!("{}-read", target.label), read);
            }
            Kind::Process => match target.path.parse::<u32>() {
                Ok(pid) => {
                    for (suffix, rights) in PROCESS_RIGHTS {
                        let label = format!("{}-{suffix}", target.label);
                        say(format_args!(
                            "probe target={label} api=win32 code={:#x}",
                            open_process(pid, rights)
                        ));
                    }
                }
                Err(e) => panic!("the probe target {} has no pid: {e}", target.label),
            },
        }
    }
    say("probe-done");
}

/// The line of an Nt open.
fn nt_line(label: &str, status: i32) {
    say(format_args!(
        "probe target={label} api=nt code={:#010x}",
        status.cast_unsigned()
    ));
}

/// The line of a std file call: the Win32 code (0 for success, `0xffffffff` for an error with
/// none, such as a marker with the wrong text).
fn win32_line(label: &str, result: std::io::Result<()>) {
    let code = match result {
        Ok(()) => 0,
        Err(e) => e.raw_os_error().map_or(u32::MAX, i32::cast_unsigned),
    };
    say(format_args!(
        "probe target={label} api=win32 code={code:#x}"
    ));
}

/// A kernel object name as `UNICODE_STRING`, with the text it points into.
struct Name {
    /// The UTF-16 text; the string points into its heap buffer, which never moves.
    units: Vec<u16>,
}

impl Name {
    /// The counted string of `units`, valid while `self` lives; `None` past 32 766 units.
    fn string(&mut self) -> Option<UNICODE_STRING> {
        let bytes = u16::try_from(self.units.len() * 2).ok()?;
        Some(UNICODE_STRING {
            Length: bytes,
            MaximumLength: bytes,
            Buffer: PWSTR(self.units.as_mut_ptr()),
        })
    }
}

/// `STATUS_NAME_TOO_LONG`, for a name no counted string holds.
const NAME_TOO_LONG: i32 = 0xC000_0106_u32.cast_signed();

/// Case-insensitive attributes for `string`, valid while it lives.
fn attributes(string: &UNICODE_STRING) -> OBJECT_ATTRIBUTES {
    OBJECT_ATTRIBUTES {
        Length: u32::try_from(size_of::<OBJECT_ATTRIBUTES>()).unwrap_or(u32::MAX),
        RootDirectory: HANDLE::default(),
        ObjectName: std::ptr::from_ref(string),
        Attributes: OBJ_CASE_INSENSITIVE,
        SecurityDescriptor: std::ptr::null(),
        SecurityQualityOfService: std::ptr::null(),
    }
}

/// Opens the object directory `name` with [`DIRECTORY_PROBE`]; the NTSTATUS. An opened handle
/// is closed at once.
fn open_directory(name: &str) -> i32 {
    let mut name = Name {
        units: name.encode_utf16().collect(),
    };
    let Some(string) = name.string() else {
        return NAME_TOO_LONG;
    };
    let attributes = attributes(&string);
    let mut out = HANDLE::default();
    // SAFETY: `attributes` points at `string`, which points into `name`; both outlive the call.
    // `out` gets the handle on success, owned and closed below.
    let status =
        unsafe { NtOpenDirectoryObject(&raw mut out, DIRECTORY_PROBE, &raw const attributes) };
    close_if_opened(status.0, out);
    status.0
}

/// Opens the pipe folder `name` (`FILE_OPEN`, a directory) with [`PIPES_PROBE`]; the NTSTATUS.
/// An opened handle is closed at once.
fn open_pipes(name: &str) -> i32 {
    let mut name = Name {
        units: name.encode_utf16().collect(),
    };
    let Some(string) = name.string() else {
        return NAME_TOO_LONG;
    };
    let attributes = attributes(&string);
    let mut out = HANDLE::default();
    let mut status_block = IO_STATUS_BLOCK::default();
    // SAFETY: `attributes` points at `string`, which points into `name`; both outlive the call.
    // The status block and `out` are written by it. No EA buffer.
    let status = unsafe {
        NtCreateFile(
            &raw mut out,
            FILE_ACCESS_RIGHTS(PIPES_PROBE),
            &raw const attributes,
            &raw mut status_block,
            None,
            FILE_FLAGS_AND_ATTRIBUTES(0),
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            FILE_OPEN,
            FILE_DIRECTORY_FILE,
            None,
            0,
        )
    };
    close_if_opened(status.0, out);
    status.0
}

/// Opens process `pid` with `rights`; the Win32 code (0 when it opened). An opened handle is
/// closed at once.
fn open_process(pid: u32, rights: PROCESS_ACCESS_RIGHTS) -> u32 {
    // SAFETY: no pointers; the handle on success is owned and closed here.
    match unsafe { OpenProcess(rights, false, pid) } {
        Ok(handle) => {
            let opened: OwnedHandle = owned(handle);
            drop(opened);
            0
        }
        Err(e) => code(&e),
    }
}

/// Closes `handle` when `status` is a success.
fn close_if_opened(status: i32, handle: HANDLE) {
    if status >= 0 && !handle.is_invalid() {
        let opened: OwnedHandle = owned(handle);
        drop(opened);
    }
}

#[test]
fn targets_round_trip() {
    let targets = vec![
        Target {
            label: "a-dir".to_owned(),
            kind: Kind::Directory,
            path: r"\Sessions\1\AppContainerNamedObjects\S-1-15-2-1".to_owned(),
        },
        Target {
            label: "a-profile".to_owned(),
            kind: Kind::Folder,
            path: r"C:\x y\profile".to_owned(),
        },
        Target {
            label: "a-chrome-4242".to_owned(),
            kind: Kind::Process,
            path: "4242".to_owned(),
        },
    ];
    assert_eq!(decode(&encode(&targets)), Ok(targets));
    assert!(decode("a-dir|dir").is_err());
    assert!(decode("a-dir|disk|C:\\").is_err());
}
