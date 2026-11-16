//! S15 `PR4i`'s VM harness: the reclaim rows (1 to 5 of the `PR4i` report) driven end to end on a
//! throwaway Windows VM, through the same crate calls the app will make once R17 wires them:
//! `UserSetup::run`, the launch's `admit`, `plan` and `apply`, `forget_matter` and
//! `UserSetup::reclaim`, with the built setup exe raising its own elevation. Windows only; a
//! development example, never built into an installer (the installer bundles `Clauseo.exe` and
//! `clauseo-sandbox-setup.exe` alone).
//!
//! **Only on a throwaway VM.** It sets up the real pool (the setup exe makes `ClauseoSbNN`
//! accounts and writes `HKLM\SOFTWARE\Clauseo\Sandbox`), so it refuses a machine already set up.
//! Everything else it makes is under `C:\ClauseoVmR\` (matters `m\one..four`, their state
//! folders, the data folder). `cleanup` removes the pool, its accounts and profiles, the group,
//! the record and `C:\ClauseoVmR\`.
//!
//! Build on Windows with the MSVC toolchain, so the setup exe carries its
//! `requireAdministrator` manifest, and put both binaries in one folder:
//!
//! ```text
//! cargo build --release -p clauseo-sandbox --bin clauseo-sandbox-setup --example s15_reclaim_vm
//! mkdir C:\vmr
//! copy target\release\clauseo-sandbox-setup.exe C:\vmr\
//! copy target\release\examples\s15_reclaim_vm.exe C:\vmr\
//! ```
//!
//! Then, as the same administrator account (UAC on, auto-elevate as on the VM), in order:
//!
//! ```text
//! 1. elevated PowerShell:  C:\vmr\s15_reclaim_vm.exe prepare
//! 2. normal PowerShell:    C:\vmr\s15_reclaim_vm.exe run        (Medium; raises the exe 4 times)
//! 3. elevated PowerShell:  C:\vmr\s15_reclaim_vm.exe row4
//! 4. elevated PowerShell:  C:\vmr\s15_reclaim_vm.exe cleanup
//! ```
//!
//! - `prepare` (elevated) makes `C:\ClauseoVmR\m\one..four` as an administrator would (so the
//!   lawyer has no `FILE_DELETE_CHILD` there: the `C:\m` layout), with an explicit entry giving
//!   the lawyer `Modify`, `WRITE_DAC` and `WRITE_OWNER` on each matter folder (inherited by what
//!   is made in it, `OICI`), and makes the lawyer the owner of `m` and of each matter folder
//!   (the folder alone).
//! - `run` (Medium, the lawyer's own token) runs set-up through the exe, launches four matters
//!   in the launcher's order (`before_launch`, `assign`, the gate, the plan, `apply`, then
//!   `repair_after_session` and `after_session` as if the agent exited at once), makes each
//!   row's state as a pool user (logged on with its password from `secrets.bin`), runs
//!   `UserSetup::reclaim()`, retries the forget and the launches, and judges rows 1, 2, 3
//!   and 5. Row 5 runs the exe directly to show its exit code.
//! - `row4` (elevated) plants another `OwnerSid` in the record, runs `--reclaim` (exit 2
//!   expected, nothing touched), puts the owner back; and reads the pool profiles' owners for
//!   row 5 (not the lawyer).
//! - `cleanup` (elevated) removes everything.
//!
//! Each row prints `ROW n PASS` or `ROW n FAIL` with its evidence: exit codes, owners and DACLs as
//! SDDL (what `icacls` shows), and the ledger's clean-ups and hand-backs owed before and after.
//! Each step ends with one `SUMMARY` line; the exit code is 0 when every row it judged passed,
//! 1 when one failed, 2 for a usage error, 3 when the step couldn't run. `--exe <path>` names the
//! setup exe when it isn't beside this one.

use std::io::Write as _;
use std::path::PathBuf;
use std::process::ExitCode;

#[cfg(windows)]
#[path = "s15_reclaim_vm/admin.rs"]
mod admin;
#[cfg(windows)]
#[path = "s15_reclaim_vm/rows.rs"]
mod rows;
#[cfg(windows)]
#[path = "s15_reclaim_vm/win.rs"]
mod win;

/// Where everything the harness makes lives.
#[cfg(windows)]
pub(crate) const ROOT: &str = r"C:\ClauseoVmR";

/// The report: lines on stdout, and the rows judged.
pub(crate) struct Out {
    step: &'static str,
    passed: Vec<String>,
    failed: Vec<String>,
    /// Lines written so far.
    lines: usize,
}

impl Out {
    fn new(step: &'static str) -> Out {
        Out {
            step,
            passed: Vec::new(),
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

    /// An evidence line under the row being judged.
    #[cfg(windows)]
    pub(crate) fn evidence(&mut self, label: &str, value: &str) {
        self.line(&format!("    {label}: {value}"));
    }

    /// Row `row` judged: PASS when `ok`, with `what` it checked.
    #[cfg(windows)]
    pub(crate) fn row(&mut self, row: &str, ok: bool, what: &str) {
        let verdict = if ok { "PASS" } else { "FAIL" };
        self.line(&format!("ROW {row} {verdict}: {what}"));
        let list = if ok {
            &mut self.passed
        } else {
            &mut self.failed
        };
        list.push(row.to_owned());
    }

    /// The step's last line, and its exit code.
    fn summary(&mut self, error: Option<&str>) -> ExitCode {
        let (pass, fail) = (self.passed.join(","), self.failed.join(","));
        let text = match error {
            Some(error) => format!(
                "SUMMARY {}: STOPPED ({error}); passed [{pass}], failed [{fail}]",
                self.step
            ),
            None => format!("SUMMARY {}: passed [{pass}], failed [{fail}]", self.step),
        };
        self.line(&text);
        match (error, self.failed.is_empty()) {
            (Some(_), _) => ExitCode::from(3),
            (None, true) => ExitCode::SUCCESS,
            (None, false) => ExitCode::from(1),
        }
    }
}

/// The step and the setup exe from the command line.
fn parse() -> Result<(String, Option<PathBuf>), String> {
    let mut args = std::env::args_os().skip(1);
    let step = args
        .next()
        .and_then(|s| s.into_string().ok())
        .ok_or("no step")?;
    let mut exe = None;
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--exe") => exe = Some(PathBuf::from(args.next().ok_or("--exe needs a path")?)),
            _ => return Err(format!("unknown argument {}", arg.to_string_lossy())),
        }
    }
    Ok((step, exe))
}

fn main() -> ExitCode {
    let mut out = Out::new("usage");
    let (step, exe) = match parse() {
        Ok(parsed) => parsed,
        Err(error) => {
            out.line(&format!("s15_reclaim_vm: {error}"));
            out.line("usage: s15_reclaim_vm <prepare|run|row4|cleanup> [--exe <path>]");
            return ExitCode::from(2);
        }
    };
    run_step(&mut out, &step, exe)
}

#[cfg(windows)]
fn run_step(out: &mut Out, step: &str, exe: Option<PathBuf>) -> ExitCode {
    let exe = exe.unwrap_or_else(default_exe);
    let result = match step {
        "prepare" => {
            out.step = "prepare";
            admin::prepare(out)
        }
        "run" => {
            out.step = "run";
            rows::run(out, &exe)
        }
        "row4" => {
            out.step = "row4";
            admin::row4(out, &exe)
        }
        "cleanup" => {
            out.step = "cleanup";
            admin::cleanup(out)
        }
        other => {
            out.line(&format!("s15_reclaim_vm: unknown step {other}"));
            return ExitCode::from(2);
        }
    };
    let error = result.err();
    out.summary(error.as_deref())
}

#[cfg(not(windows))]
fn run_step(out: &mut Out, _step: &str, _exe: Option<PathBuf>) -> ExitCode {
    out.line("s15_reclaim_vm: Windows only (S15 PR4i's VM harness)");
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
