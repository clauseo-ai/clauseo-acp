//! The S15 PR5 harness: proves tier B's runner (an agent started as its matter's pool user under
//! the restricted Low token) on a Windows VM step by step, and unattended in CI on
//! `windows-11-arm`. See `s15_launch_vm/README.md` for how to run it.
//!
//! ```text
//! s15_launch_vm prepare|rows|cleanup|row <name> [options]   (step mode, the VM)
//! s15_launch_vm --unattended [options]                       (CI: prepare, every row, cleanup)
//! s15_launch_vm probe <what> ...                             (run inside the sandbox)
//! ```
//!
//! Unattended prints one `PASS <row>`, `FAIL <row>: <why>` or `SKIP <row>: <why>` line per row,
//! runs cleanup even when a row fails, and exits 1 when any row (or prepare) failed. Each row
//! has a hard limit (`--row-timeout`, 120 s by default). Windows only; elsewhere it exits 2.

use std::process::ExitCode;

#[cfg(windows)]
fn main() -> ExitCode {
    harness::main()
}

#[cfg(not(windows))]
fn main() -> ExitCode {
    use std::io::Write as _;
    let line = "s15_launch_vm: tier B's runner harness runs on Windows only\n";
    if std::io::stderr().write_all(line.as_bytes()).is_err() {
        return ExitCode::from(3);
    }
    ExitCode::from(2)
}

#[cfg(windows)]
#[path = "s15_launch_vm/checks.rs"]
mod checks;
#[cfg(windows)]
#[path = "s15_launch_vm/checks_m3.rs"]
mod checks_m3;
#[cfg(windows)]
#[path = "s15_launch_vm/checks_sec.rs"]
mod checks_sec;
#[cfg(windows)]
#[path = "s15_launch_vm/checks_ui.rs"]
mod checks_ui;
#[cfg(windows)]
#[path = "s15_launch_vm/cli.rs"]
mod cli;
#[cfg(windows)]
#[path = "s15_launch_vm/codex.rs"]
mod codex;
#[cfg(windows)]
#[path = "s15_launch_vm/launch.rs"]
mod launch;
#[cfg(windows)]
#[path = "s15_launch_vm/manifest.rs"]
mod manifest;
#[cfg(windows)]
#[path = "s15_launch_vm/prepare.rs"]
mod prepare;
#[cfg(windows)]
#[path = "s15_launch_vm/probe.rs"]
mod probe;
#[cfg(windows)]
#[path = "s15_launch_vm/probe_sec.rs"]
mod probe_sec;
#[cfg(windows)]
#[path = "s15_launch_vm/rows.rs"]
mod rows;
#[cfg(windows)]
#[path = "s15_launch_vm/sys.rs"]
mod sys;
#[cfg(windows)]
#[path = "s15_launch_vm/ui.rs"]
mod ui;
#[cfg(windows)]
#[path = "s15_launch_vm/win.rs"]
mod win;
#[cfg(windows)]
#[path = "s15_launch_vm/win_desk.rs"]
mod win_desk;
#[cfg(windows)]
#[path = "s15_launch_vm/win_sec.rs"]
mod win_sec;

#[cfg(windows)]
mod harness {
    use std::process::ExitCode;

    use super::cli::{self, Opts, Step};
    use super::manifest::Manifest;
    use super::rows::{Outcome, ROWS, run_row};
    use super::sys::say;
    use super::{prepare, probe};

    pub(super) fn main() -> ExitCode {
        let args: Vec<String> = std::env::args().skip(1).collect();
        if args.first().is_some_and(|a| a == "probe") {
            return probe::run(args.get(1..).unwrap_or_default());
        }
        let mut opts = match cli::parse(&args) {
            Ok(opts) => opts,
            Err(why) => {
                say(&format!("{why}\n{}", cli::USAGE));
                return ExitCode::from(2);
            }
        };
        match std::path::absolute(&opts.root) {
            Ok(root) => opts.root = root,
            Err(e) => {
                say(&format!("--root: {e}"));
                return ExitCode::from(2);
            }
        }
        let ok = match &opts.step {
            Step::Prepare => match prepare::prepare(&opts) {
                Ok(_) => true,
                Err(why) => {
                    say(&format!(
                        "prepare failed: {why} (cleanup removes what was made)"
                    ));
                    false
                }
            },
            Step::Row(name) => with_manifest(&opts, |m| rows(&opts, m, &[name.as_str()])),
            Step::Rows => with_manifest(&opts, |m| rows(&opts, m, &ROWS)),
            Step::Cleanup => prepare::cleanup(&opts.root),
            Step::Unattended => unattended(&opts),
        };
        if ok {
            ExitCode::SUCCESS
        } else {
            ExitCode::from(1)
        }
    }

    fn with_manifest(opts: &Opts, then: impl FnOnce(&Manifest) -> bool) -> bool {
        match Manifest::load(&opts.root) {
            Ok(Some(manifest)) => then(&manifest),
            Ok(None) => {
                say(&format!(
                    "no manifest in {}: run prepare",
                    opts.root.display()
                ));
                false
            }
            Err(e) => {
                say(&format!("the manifest can't be read: {e}"));
                false
            }
        }
    }

    /// Runs `names`; whether none failed. Prints a tally.
    fn rows(opts: &Opts, manifest: &Manifest, names: &[&str]) -> bool {
        let mut failed = Vec::new();
        let mut skipped = Vec::new();
        for name in names {
            match run_row(name, manifest, opts.row_timeout) {
                Outcome::Pass(_) => {}
                Outcome::Fail(_) => failed.push(*name),
                Outcome::Skip(_) => skipped.push(*name),
            }
        }
        say(&format!(
            "rows: {} run, failed {failed:?}, skipped {skipped:?}",
            names.len()
        ));
        failed.is_empty()
    }

    fn unattended(opts: &Opts) -> bool {
        say("unattended: prepare, every row, cleanup");
        let ok = match prepare::prepare(opts) {
            Ok(manifest) => rows(opts, &manifest, &ROWS),
            Err(why) => {
                say(&format!("FAIL prepare: {why}"));
                false
            }
        };
        let clean = prepare::cleanup(&opts.root);
        if !clean {
            say("cleanup: something stayed (above)");
        }
        ok && clean
    }
}
