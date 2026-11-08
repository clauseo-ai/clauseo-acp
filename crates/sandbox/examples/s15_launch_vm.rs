//! The S15 PR5 harness: proves tier B's runner (an agent started as its matter's pool user under
//! the restricted Low token) on a Windows VM step by step, and unattended in CI on
//! `windows-11-arm`. See `s15_launch_vm/README.md` for how to run it.
//!
//! ```text
//! s15_launch_vm prepare|rows|cleanup|row <name> [options]   (step mode, the VM)
//! s15_launch_vm --unattended [options]                       (CI: prepare, every row, cleanup)
//! s15_launch_vm row node-pipes --interactive-desktop          (a desktop session: strict UI gate)
//! s15_launch_vm probe <what> ...                             (run inside the sandbox)
//! ```
//!
//! Each row prints one verdict line: `PASS <row>`, `FAIL <row>: <why>`, `SKIP <row>: <why>` or
//! `INFO <row>: <why>` (a positive control couldn't run, so the row proves nothing). Unattended
//! runs cleanup even when a row fails. Every run of rows (`row`, `rows`, `--unattended`) ends
//! with a `result: green` or `result: red (…)` line and exits 1 when any row is FAIL or INFO, or
//! when prepare or cleanup failed: an INFO row turns CI red, in step mode too. SKIP doesn't. Each
//! row has a hard limit (`--row-timeout`, 120 s by default). Windows only; elsewhere it exits 2.

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
#[path = "s15_launch_vm/cancel_verdict.rs"]
mod cancel_verdict;
#[cfg(windows)]
#[path = "s15_launch_vm/checks.rs"]
mod checks;
#[cfg(windows)]
#[path = "s15_launch_vm/checks_cancel.rs"]
mod checks_cancel;
#[cfg(windows)]
#[path = "s15_launch_vm/checks_cleanup.rs"]
mod checks_cleanup;
#[cfg(windows)]
#[path = "s15_launch_vm/checks_ctl.rs"]
mod checks_ctl;
#[cfg(windows)]
#[path = "s15_launch_vm/checks_m3.rs"]
mod checks_m3;
#[cfg(windows)]
#[path = "s15_launch_vm/checks_pool.rs"]
mod checks_pool;
#[cfg(windows)]
#[path = "s15_launch_vm/checks_sec.rs"]
mod checks_sec;
#[cfg(windows)]
#[path = "s15_launch_vm/checks_ui.rs"]
mod checks_ui;
#[cfg(windows)]
#[path = "s15_launch_vm/cleanup_verdict.rs"]
mod cleanup_verdict;
#[cfg(windows)]
#[path = "s15_launch_vm/cli.rs"]
mod cli;
#[cfg(windows)]
#[path = "s15_launch_vm/codex.rs"]
mod codex;
#[cfg(windows)]
#[path = "s15_launch_vm/gate.rs"]
mod gate;
#[cfg(windows)]
#[path = "s15_launch_vm/launch.rs"]
mod launch;
#[cfg(windows)]
#[path = "s15_launch_vm/manifest.rs"]
mod manifest;
#[cfg(windows)]
#[path = "s15_launch_vm/pool_verdict.rs"]
mod pool_verdict;
#[cfg(windows)]
#[path = "s15_launch_vm/prepare.rs"]
mod prepare;
#[cfg(windows)]
#[path = "s15_launch_vm/probe.rs"]
mod probe;
#[cfg(windows)]
#[path = "s15_launch_vm/probe_items.rs"]
mod probe_items;
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
#[path = "s15_launch_vm/token_verdict.rs"]
mod token_verdict;
#[cfg(windows)]
#[path = "s15_launch_vm/ui.rs"]
mod ui;
#[cfg(windows)]
#[path = "s15_launch_vm/win.rs"]
mod win;
#[cfg(windows)]
#[path = "s15_launch_vm/win_ctl.rs"]
mod win_ctl;
#[cfg(windows)]
#[path = "s15_launch_vm/win_desk.rs"]
mod win_desk;
#[cfg(windows)]
#[path = "s15_launch_vm/win_logon.rs"]
mod win_logon;
#[cfg(windows)]
#[path = "s15_launch_vm/win_pool.rs"]
mod win_pool;
#[cfg(windows)]
#[path = "s15_launch_vm/win_sec.rs"]
mod win_sec;
#[cfg(windows)]
#[path = "s15_launch_vm/win_suspend.rs"]
mod win_suspend;
#[cfg(windows)]
#[path = "s15_launch_vm/win_switch.rs"]
mod win_switch;
#[cfg(windows)]
#[path = "s15_launch_vm/win_thread.rs"]
mod win_thread;

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
            Step::Row(name) => with_manifest(&opts, |m| verdict(&rows(&opts, m, &[name.as_str()]))),
            Step::Rows => with_manifest(&opts, |m| verdict(&rows(&opts, m, &ROWS))),
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

    /// Prints the final line, `result: green` or `result: red (<what>)` naming every FAIL and
    /// INFO; whether it is green.
    fn verdict(red: &[String]) -> bool {
        if red.is_empty() {
            say("result: green (no row FAIL or INFO)");
            return true;
        }
        say(&format!(
            "result: red ({}; INFO: a positive control couldn't run, so the row proves nothing)",
            red.join(", ")
        ));
        false
    }

    /// Runs `names` and prints a tally; what turns the run red: `FAIL <row>` and `INFO <row>`.
    fn rows(opts: &Opts, manifest: &Manifest, names: &[&str]) -> Vec<String> {
        let mut failed = Vec::new();
        let mut skipped = Vec::new();
        let mut info = Vec::new();
        for name in names {
            match run_row(name, manifest, opts.row_timeout, opts.interactive_desktop) {
                Outcome::Pass(_) => {}
                Outcome::Fail(_) => failed.push(*name),
                Outcome::Skip(_) => skipped.push(*name),
                Outcome::Info(_) => info.push(*name),
            }
        }
        say(&format!(
            "rows: {} run, failed {failed:?}, skipped {skipped:?}, info {info:?}",
            names.len()
        ));
        let red = failed.iter().map(|name| format!("FAIL {name}"));
        red.chain(info.iter().map(|name| format!("INFO {name}")))
            .collect()
    }

    fn unattended(opts: &Opts) -> bool {
        say("unattended: prepare, every row, cleanup");
        let mut red = match prepare::prepare(opts) {
            Ok(manifest) => rows(opts, &manifest, &ROWS),
            Err(why) => {
                say(&format!("FAIL prepare: {why}"));
                vec!["FAIL prepare".to_owned()]
            }
        };
        if !prepare::cleanup(&opts.root) {
            say("cleanup: something stayed (above)");
            red.push("cleanup left something".to_owned());
        }
        verdict(&red)
    }
}
