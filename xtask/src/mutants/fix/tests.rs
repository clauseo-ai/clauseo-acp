//! The text of each fix of `cargo xtask mutants`.

use super::*;
use std::collections::HashSet;
use std::mem::discriminant;

/// The text of every fix, by an exhaustive match: a new variant does not compile until its text
/// is pinned here. The samples are one per variant.
#[test]
fn every_fix_has_its_exact_text() {
    let samples = [
        Fix::Missed,
        Fix::Split,
        Fix::Setup { pinned: "1.2.3" },
        Fix::GitNotStarted,
        Fix::GitFailed {
            command: "git diff".into(),
        },
        Fix::GitTimedOut,
        Fix::Write {
            path: "target/xtask/mutants.diff",
        },
        Fix::ByHand {
            cmd: "cargo mutants".into(),
        },
        Fix::Unparsed,
        Fix::Baseline,
        Fix::Unexpected {
            log: "target/xtask/mutants.out/log/x.log".into(),
        },
        Fix::Slow,
        Fix::Unviable,
        Fix::RemoveByHand,
    ];
    let kinds: HashSet<_> = samples.iter().map(discriminant).collect();
    assert_eq!(kinds.len(), samples.len(), "two samples share a variant");
    for fix in &samples {
        let expected = match fix {
            Fix::Missed => {
                "add a test that fails on this change, or exclude it with a reason in \
                 .cargo/mutants.toml (the code owner approves it)"
            }
            Fix::Split => "split the change: cargo xtask mutants caps a run at 20 minutes",
            Fix::Setup { .. } => {
                "run `cargo xtask setup`, which installs cargo-mutants 1.2.3, the version \
                 xtask/src/setup.rs pins"
            }
            Fix::GitNotStarted => "install git, or put it on PATH",
            Fix::GitFailed { .. } => {
                "run `git diff` by hand to see why it fails; `git fetch origin main` fetches a \
                 missing origin/main"
            }
            Fix::GitTimedOut => "run the git command above by hand to see why it does not finish",
            Fix::Write { .. } => {
                "make target/xtask/mutants.diff writable (check the disk has room), then run \
                 cargo xtask mutants again"
            }
            Fix::ByHand { .. } => "run `cargo mutants` by hand to see what goes wrong",
            Fix::Unparsed => {
                "run cargo xtask mutants again; if it fails the same way, make \
                 xtask/src/mutants/outcomes.rs read the outcomes.json of the cargo-mutants \
                 version setup pins"
            }
            Fix::Baseline => {
                "read the log: the build and tests must pass in a copy of the tree that holds \
                 .git but not what .gitignore excludes, as .cargo/mutants.toml sets"
            }
            Fix::Unexpected { .. } => {
                "read target/xtask/mutants.out/log/x.log, then run cargo xtask mutants again"
            }
            Fix::Slow => {
                "none when the change makes the code loop forever, which the tests then catch; \
                 if the tests were only slow on a busy machine, run cargo xtask mutants again"
            }
            Fix::Unviable => {
                "none: a mutant that does not compile cannot be tested, and the compiler \
                 already refuses that change"
            }
            Fix::RemoveByHand => "delete that folder by hand; nothing else uses it",
        };
        assert_eq!(fix.to_string(), expected);
    }
}
