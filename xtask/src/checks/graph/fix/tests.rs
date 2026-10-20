//! Pins the text of every `graph::Fix` variant. The `match` has no `_` arm, so a new variant
//! does not compile until it has a pinned text here.

use super::Fix;
use crate::process::{Cmd, Program, TimeoutFix};
use std::collections::HashSet;
use std::mem::discriminant;
use std::time::Duration;

fn pinned(fix: &Fix) -> String {
    match fix {
        Fix::TreeParse => "update `Tree::parse` in xtask/src/checks/graph.rs to the lines this cargo \
             prints, since the check reads each crate's dependency graph from them"
            .to_owned(),
        Fix::CargoOnPath => {
            "put `cargo` on PATH, since this check reads the dependency graph from cargo".to_owned()
        }
        Fix::TimedOut(_) => "run `cargo tree -p x` by hand to see why it does not finish within 5 min"
            .to_owned(),
        Fix::Lockfile => "Cargo.lock does not match the manifests, and the check never rewrites it. \
             Run `cargo update --workspace` to bring it in line, then commit Cargo.lock"
            .to_owned(),
        Fix::Fetch => "a crate is not downloaded and the check runs offline. Run `cargo fetch \
             --locked`, then run the check again"
            .to_owned(),
        Fix::Manifest => "cargo could not resolve the dependency that its error above names. Correct \
             that dependency in the manifest of the package the error says requires it, then run \
             the check again"
            .to_owned(),
        Fix::Either => "cargo could not resolve the dependency that its error above names. If \
             Cargo.lock lists that package, it is not downloaded and the check runs offline, so run \
             `cargo fetch --locked`. If it does not, correct that dependency in the manifest of \
             the package the error says requires it. Then run the check again"
            .to_owned(),
        Fix::CargoReports { command } => format!(
            "fix what cargo reports above until `{command}` succeeds, since this check reads the \
             dependency graph from it"
        ),
    }
}

#[test]
fn every_fix_prints_its_pinned_text() {
    let samples = [
        Fix::TreeParse,
        Fix::CargoOnPath,
        Fix::TimedOut(TimeoutFix::of(&Cmd::new(
            Program::Cargo,
            ["tree", "-p", "x"],
            Duration::from_mins(5),
        ))),
        Fix::Lockfile,
        Fix::Fetch,
        Fix::Manifest,
        Fix::Either,
        Fix::CargoReports {
            command: "cargo tree -p clauseo-host".to_owned(),
        },
    ];
    let kinds: HashSet<_> = samples.iter().map(discriminant).collect();
    assert_eq!(
        kinds.len(),
        samples.len(),
        "two samples are the same variant"
    );
    for fix in &samples {
        assert_eq!(fix.to_string(), pinned(fix));
    }
}

#[test]
fn the_cargo_reports_fix_names_the_command_that_failed() {
    let fix = Fix::CargoReports {
        command: "cargo tree -p clauseo-host --locked".to_owned(),
    };
    assert_eq!(
        fix.to_string(),
        "fix what cargo reports above until `cargo tree -p clauseo-host --locked` succeeds, since \
         this check reads the dependency graph from it"
    );
}
