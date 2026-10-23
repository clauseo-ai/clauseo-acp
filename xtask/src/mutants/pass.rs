//! The passes of `cargo xtask mutants` and how their results add up.
//!
//! The main pass mutates every package the diff touches, without features: a feature in
//! `.cargo/mutants.toml` would go to every mutant's build, and cargo refuses a feature of protocol
//! for any other package, so each of their mutants would come back unviable. Only
//! `crates/protocol/src/wire.rs` needs protocol's `specta` feature (`WireFormat` and its tests are
//! built only with it), so the main pass leaves that file out, and a second pass mutates it alone
//! with the feature on, only when the diff touches it. Both passes share the one cap.

use super::outcomes::Counts;
use super::{DIFF, JOBS, MUTANTS_KILL_GRACE, MUTANTS_TIMEOUT, OUTCOMES, OUTPUT_DIR, OUTPUT_PARENT};
use crate::process::{Cmd, Program};
use std::time::Duration;

/// The one file whose mutants need protocol's `specta` feature.
pub(super) const WIRE: &str = "crates/protocol/src/wire.rs";
/// The folder the wire pass makes its `mutants.out` in (`--output`).
pub(super) const WIRE_OUTPUT_PARENT: &str = "target/xtask/wire";
/// The wire pass's output folder, which each run removes first.
pub(super) const WIRE_OUTPUT_DIR: &str = "target/xtask/wire/mutants.out";
/// What xtask reads of it.
pub(super) const WIRE_OUTCOMES: &str = "target/xtask/wire/mutants.out/outcomes.json";

/// One run of cargo-mutants: its command and where it writes.
pub(super) struct Pass {
    /// What the summary line calls it when there are two passes.
    pub label: &'static str,
    pub cmd: Cmd,
    /// The `mutants.out` cargo-mutants makes in its `--output` folder.
    pub output_dir: &'static str,
    /// Its `outcomes.json`.
    pub outcomes: &'static str,
}

impl Pass {
    /// The main pass: every package the diff touches, without features, and without `WIRE`. It
    /// may use the whole cap.
    pub(super) fn main() -> Pass {
        let args = [
            "mutants",
            // Mutate every package the diff touches; from a virtual workspace's root
            // cargo-mutants would otherwise pick none.
            "--workspace",
            "--exclude",
            WIRE,
        ];
        Pass {
            label: "everything but crates/protocol/src/wire.rs",
            cmd: cmd(&args, OUTPUT_PARENT, MUTANTS_TIMEOUT),
            output_dir: OUTPUT_DIR,
            outcomes: OUTCOMES,
        }
    }

    /// The wire pass: `WIRE` alone, with protocol's `specta` feature, for `timeout`, what the main
    /// pass left of the cap.
    pub(super) fn wire(timeout: Duration) -> Pass {
        let args = [
            "mutants",
            "--package",
            "clauseo-protocol",
            "--features",
            "clauseo-protocol/specta",
            "--file",
            WIRE,
        ];
        Pass {
            label: "crates/protocol/src/wire.rs with protocol's specta feature",
            cmd: cmd(&args, WIRE_OUTPUT_PARENT, timeout),
            output_dir: WIRE_OUTPUT_DIR,
            outcomes: WIRE_OUTCOMES,
        }
    }
}

/// `cargo` with `args`, then the diff, `--output output_parent` and the jobs every pass shares.
fn cmd(args: &[&str], output_parent: &str, timeout: Duration) -> Cmd {
    let shared = ["--in-diff", DIFF, "--output", output_parent, "--jobs", JOBS];
    Cmd::new(Program::Cargo, args.iter().chain(&shared), timeout).kill_grace(MUTANTS_KILL_GRACE)
}

/// Whether `diff` (`git diff` output) changes `WIRE`: one of its `diff --git` lines names it.
pub(super) fn touches_wire(diff: &str) -> bool {
    let wire = format!(" b/{WIRE}");
    diff.lines()
        .filter_map(|line| line.strip_prefix("diff --git "))
        .any(|files| files.ends_with(&wire))
}

/// What is left of `MUTANTS_TIMEOUT` after `spent`, in whole seconds, or `None` when less than a
/// second is.
pub(super) fn remaining(spent: Duration) -> Option<Duration> {
    let left = MUTANTS_TIMEOUT.checked_sub(spent)?;
    let whole = Duration::from_secs(left.as_secs());
    (!whole.is_zero()).then_some(whole)
}

/// How a pass that exited came out.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Judged {
    /// cargo-mutants found no mutant in the changed lines, so it wrote no outcomes.
    NoMutant,
    /// It failed before there was anything to count; the failure is printed.
    Failed,
    /// Its outcomes, counted; `passed` is false when one was missed or anything failed.
    Counted { passed: bool, counts: Counts },
}

impl Judged {
    /// Whether nothing was missed and nothing failed.
    pub(super) fn passed(&self) -> bool {
        match self {
            Judged::NoMutant => true,
            Judged::Failed => false,
            Judged::Counted { passed, .. } => *passed,
        }
    }

    /// Its counts; none for a pass that counted nothing.
    pub(super) fn counts(&self) -> Counts {
        match self {
            Judged::Counted { counts, .. } => counts.clone(),
            Judged::NoMutant | Judged::Failed => Counts::default(),
        }
    }

    /// What its summary line says after the `mutants ...: `.
    pub(super) fn summary(&self) -> String {
        match self {
            Judged::NoMutant => "cargo-mutants found no mutant in the changed lines".to_owned(),
            Judged::Failed => "it failed before it tested any mutant".to_owned(),
            Judged::Counted { counts, .. } => counts.to_string(),
        }
    }
}

/// `passed` as the summary line says it.
pub(super) fn status(passed: bool) -> &'static str {
    if passed { "ok" } else { "FAILED" }
}
