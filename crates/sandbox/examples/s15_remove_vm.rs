//! S15 `PR4j`'s VM harness: the setup exe's `--remove` driven directly on a throwaway Windows VM,
//! as a non-owner (refused, nothing changed), as the owner (per-user: the pool goes) and as
//! LOCAL SYSTEM with `--per-machine` (every pool goes), each judged from a listing taken before
//! and after. Windows only (`compare` runs anywhere); a development example, never built into
//! an installer (the installer bundles `Clauseo.exe` and `clauseo-sandbox-setup.exe` alone).
//!
//! **Only on a throwaway VM.** `prepare` sets up the real pool (the setup exe makes `ClauseoSbNN`
//! accounts and writes `HKLM\SOFTWARE\Clauseo\Sandbox`), so it refuses a machine already set up.
//! Everything else it makes is under `C:\ClauseoVmX\` (the data folder, `matters\one` and
//! `matters\two` with a few files and a nested folder each, and their state folders). `cleanup`
//! removes the pool's accounts and profiles, `ClauseoVmOther`, the group, the record and
//! `C:\ClauseoVmX\`.
//!
//! Build on Windows with the MSVC toolchain, so the setup exe carries its
//! `requireAdministrator` manifest. Make `C:\vmx` from a **normal** PowerShell (so the lawyer
//! owns it and writes the listings there) and put both binaries in it:
//!
//! ```text
//! cargo build --release -p clauseo-sandbox --bin clauseo-sandbox-setup --example s15_remove_vm
//! mkdir C:\vmx
//! copy target\release\clauseo-sandbox-setup.exe C:\vmx\
//! copy target\release\examples\s15_remove_vm.exe C:\vmx\
//! ```
//!
//! The lawyer is the VM's administrator account (UAC on), in a normal PowerShell (Medium) unless
//! a step says elevated. `PsExec64.exe` (Sysinternals) is in `C:\vmx` for (c). In order:
//!
//! ```text
//! 0. elevated:  net user ClauseoVmOther Vm-Other-1! /add       (a second standard user)
//!    normal:    runas /user:ClauseoVmOther "cmd /c exit"       (its first logon makes its
//!                                                              profile now, not between listings)
//! (a) non-owner
//! 1. normal:    C:\vmx\s15_remove_vm.exe prepare               (one UAC prompt: the set-up)
//! 2. normal:    C:\vmx\s15_remove_vm.exe snapshot C:\vmx\a-before.txt
//! 3. normal:    runas /user:ClauseoVmOther powershell
//!    in that ClauseoVmOther window:
//!               C:\vmx\s15_remove_vm.exe remove                (UAC asks for an administrator:
//!                                                              type the lawyer's credentials;
//!                                                              exit 0, Done, nothing touched)
//! 4. normal:    C:\vmx\s15_remove_vm.exe snapshot C:\vmx\a-after.txt
//!               C:\vmx\s15_remove_vm.exe compare C:\vmx\a-before.txt C:\vmx\a-after.txt nonowner
//! (b) owner, per-user
//! 5. normal:    C:\vmx\s15_remove_vm.exe remove                (UAC prompt; exit 0, Done)
//!               C:\vmx\s15_remove_vm.exe snapshot C:\vmx\b-after.txt
//!               C:\vmx\s15_remove_vm.exe compare C:\vmx\a-after.txt C:\vmx\b-after.txt owner
//! (c) per-machine as LOCAL SYSTEM
//! 6. elevated:  C:\vmx\s15_remove_vm.exe cleanup
//!    normal:    C:\vmx\s15_remove_vm.exe prepare
//!               C:\vmx\s15_remove_vm.exe snapshot C:\vmx\c-before.txt
//! 7. elevated:  cd C:\vmx
//!    .\PsExec64.exe -accepteula -s C:\vmx\clauseo-sandbox-setup.exe --remove --per-machine
//!                                                              ("exited with error code 0")
//! 8. normal:    C:\vmx\s15_remove_vm.exe snapshot C:\vmx\c-after.txt
//!               C:\vmx\s15_remove_vm.exe compare C:\vmx\c-before.txt C:\vmx\c-after.txt machine
//! 9. elevated:  C:\vmx\s15_remove_vm.exe cleanup
//! ```
//!
//! - `prepare` (normal: the lawyer's Medium token, as the app runs) refuses a machine already set
//!   up, or one where `C:\ClauseoVmX` is left over. It makes the data folder (so it is the
//!   lawyer's own), the matters and their files, runs set-up through the exe as the app does
//!   (`UserSetup`, `--set-up --parent <this pid> --data-dir C:\ClauseoVmX\data`: four pool users,
//!   their `LocalLow` and `DeviceSync` denies), then launches each matter in the launcher's
//!   order: `before_launch`, `MatterSids::assign`, `admit`, `plan`, `apply` through the lawyer's
//!   `NtfsAcl`, `repair_after_session`, `after_session`. So the matters carry real pool entries,
//!   Low labels and fences. Each matter folder is the lawyer's, with an explicit entry holding
//!   `WRITE_OWNER` (`0x1f01bf`), which the label needs. Until main's pool-reuse fix, matter two's
//!   `assign` finds every pool user held back by matter one's entries: its launch then applies
//!   the same plan as an unused pool user without `assign`, so matter two is fenced but not
//!   registered in `matters.json` (its `launch two` line says which happened).
//! - `snapshot <file>` (normal, the lawyer) writes a sorted listing: every file and folder under
//!   the matters and state folders (size, SHA-256, owner, SDDL with the label); the lawyer's
//!   profile folder, `AppData` and each folder in it as SDDL; `DeviceSync`'s SDDL; every local
//!   user and group (`Get-LocalUser`, `Get-LocalGroup`, the members' SIDs); the profile folders
//!   in `C:\Users`; the record, present or absent, and its values (`reg query`).
//! - `remove [--per-machine]` (any) starts the exe with `--remove` as the uninstall hook does
//!   (verb `open`, so its manifest raises the prompt), waits, and prints its exit code.
//! - `compare <before> <after> <nonowner|owner|machine>` (any, any OS) prints every difference in
//!   four groups (content, entries and labels, accounts and groups, record), then `PASS` or
//!   `FAIL` for the mode. `nonowner`: nothing changed at all. `owner`: no content change, the
//!   lawyer's profile, accounts and groups unchanged, and the only entries removed are the pool
//!   users', the group's and the Low labels, with none of them left; the `ClauseoSb` accounts,
//!   their profiles, the group and the record gone. `machine`: the same, except that entries and
//!   labels naming the removed SIDs may stay where `ledger_work.rs` says the administrator's
//!   path leaves them (it removes only the pool allows and the label on each matter folder, with
//!   no walk); they are listed, not failed, and each matter folder's own pool allow and label
//!   must be gone.
//! - `cleanup` (elevated) removes everything; each part is tried whatever the last did.
//!
//! The setup exe's exits (`SetupExit`): 0 done (for `--remove`, removed, or nothing of the
//! asker's to remove), 1 usage, 3 failed (nothing touched), 4 removed partly, 5 lock held. Each
//! step ends with one line that starts `PASS`, `FAIL` or `STOPPED`; the exit code is 0, 1, or 3
//! (2 for a usage error). `--exe <path>` names the setup exe when it isn't beside this one.

use std::io::Write as _;
use std::path::PathBuf;
use std::process::ExitCode;

#[cfg(windows)]
#[path = "s15_remove_vm/admin.rs"]
mod admin;
#[path = "s15_remove_vm/compare.rs"]
mod compare;
#[cfg(windows)]
#[path = "s15_remove_vm/prepare.rs"]
mod prepare;
#[cfg(windows)]
#[path = "s15_remove_vm/snapshot.rs"]
mod snapshot;
#[cfg(windows)]
#[path = "s15_remove_vm/win.rs"]
mod win;

/// Where everything the harness makes lives.
pub(crate) const ROOT: &str = r"C:\ClauseoVmX";
/// The second standard user the non-owner run uses.
#[cfg(windows)]
pub(crate) const OTHER_USER: &str = "ClauseoVmOther";

/// The report: lines on stdout, and the checks judged.
pub(crate) struct Out {
    step: String,
    failed: Vec<String>,
    /// Lines written so far.
    lines: usize,
}

impl Out {
    fn new(step: &str) -> Out {
        Out {
            step: step.to_owned(),
            failed: Vec::new(),
            lines: 0,
        }
    }

    /// One line.
    pub(crate) fn line(&mut self, text: &str) {
        self.lines += 1;
        let mut stdout = std::io::stdout().lock();
        if writeln!(stdout, "{text}").is_err() {
            // stdout is gone: nothing left to tell.
        }
    }

    /// An evidence line under the check being judged.
    pub(crate) fn evidence(&mut self, label: &str, value: &str) {
        self.line(&format!("    {label}: {value}"));
    }

    /// A check: `ok` or not, with `what` it checked.
    pub(crate) fn check(&mut self, ok: bool, what: &str) {
        let verdict = if ok { "ok" } else { "NOT OK" };
        self.line(&format!("  {verdict}: {what}"));
        if !ok {
            self.failed.push(what.to_owned());
        }
    }

    /// The step's last line, and its exit code.
    fn summary(&mut self, error: Option<&str>) -> ExitCode {
        let step = self.step.clone();
        let (text, code) = match (error, self.failed.is_empty()) {
            (Some(error), _) => (format!("STOPPED {step}: {error}"), ExitCode::from(3)),
            (None, true) => (format!("PASS {step}"), ExitCode::SUCCESS),
            (None, false) => (
                format!("FAIL {step}: {}", self.failed.join("; ")),
                ExitCode::from(1),
            ),
        };
        self.line(&text);
        code
    }
}

/// The command line: the step, its words, and `--exe`.
struct Args {
    step: String,
    words: Vec<String>,
    exe: Option<PathBuf>,
}

fn parse() -> Result<Args, String> {
    let mut args = std::env::args_os().skip(1);
    let step = args
        .next()
        .and_then(|s| s.into_string().ok())
        .ok_or("no step")?;
    let mut words = Vec::new();
    let mut exe = None;
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--exe") => exe = Some(PathBuf::from(args.next().ok_or("--exe needs a path")?)),
            Some(word) => words.push(word.to_owned()),
            None => return Err(format!("{} isn't text", arg.to_string_lossy())),
        }
    }
    Ok(Args { step, words, exe })
}

const USAGE: &str = "usage: s15_remove_vm prepare | snapshot <file> | remove [--per-machine] | \
                     compare <before> <after> <nonowner|owner|machine> | cleanup [--exe <path>]";

fn main() -> ExitCode {
    let mut out = Out::new("usage");
    let args = match parse() {
        Ok(args) => args,
        Err(error) => {
            out.line(&format!("s15_remove_vm: {error}"));
            out.line(USAGE);
            return ExitCode::from(2);
        }
    };
    if args.step == "compare" {
        let [before, after, mode] = args.words.as_slice() else {
            out.line(USAGE);
            return ExitCode::from(2);
        };
        let Some(mode) = compare::Mode::parse(mode) else {
            out.line(USAGE);
            return ExitCode::from(2);
        };
        out.step = format!("compare {}", mode.name());
        let result = compare::compare(&mut out, before.as_ref(), after.as_ref(), mode);
        let error = result.err();
        return out.summary(error.as_deref());
    }
    run_step(&mut out, &args)
}

#[cfg(windows)]
fn run_step(out: &mut Out, args: &Args) -> ExitCode {
    let exe = args.exe.clone().unwrap_or_else(default_exe);
    out.step.clone_from(&args.step);
    let result = match (args.step.as_str(), args.words.as_slice()) {
        ("prepare", []) => prepare::prepare(out, &exe),
        ("snapshot", [file]) => snapshot::snapshot(out, file.as_ref()),
        ("remove", []) => admin::remove(out, &exe, false),
        ("remove", [flag]) if flag == "--per-machine" => admin::remove(out, &exe, true),
        ("cleanup", []) => admin::cleanup(out),
        _ => {
            out.line(USAGE);
            return ExitCode::from(2);
        }
    };
    let error = result.err();
    out.summary(error.as_deref())
}

#[cfg(not(windows))]
fn run_step(out: &mut Out, args: &Args) -> ExitCode {
    out.step.clone_from(&args.step);
    if args.exe.is_some() {
        out.line("s15_remove_vm: --exe is for the Windows steps");
    }
    out.line("s15_remove_vm: Windows only, but for compare (S15 PR4j's VM harness)");
    out.summary(Some("not Windows"))
}

/// `clauseo-sandbox-setup.exe` beside this exe, else in the folder above (`target\release`
/// when this one runs from `target\release\examples`).
#[cfg(windows)]
fn default_exe() -> PathBuf {
    let name = "clauseo-sandbox-setup.exe";
    let here = std::env::current_exe().unwrap_or_default();
    let beside = here.with_file_name(name);
    if beside.exists() {
        return beside;
    }
    here.parent()
        .and_then(std::path::Path::parent)
        .map_or(beside, |up| up.join(name))
}
