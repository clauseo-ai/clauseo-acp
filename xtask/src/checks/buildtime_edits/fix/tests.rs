//! Pins the text of every `buildtime_edits::Fix` variant. The `match` has no `_` arm, so a new
//! variant does not compile until it has a pinned text here.

use super::Fix;
use std::collections::HashSet;
use std::mem::discriminant;

fn pinned(fix: &Fix) -> &'static str {
    match fix {
        Fix::Leftover => {
            "run `cargo xtask buildtime web` once to restore the file from target/xtask/buildtime, \
             or remove the attribute by hand"
        }
        Fix::LiveRun => "rerun the check when that run ends",
    }
}

#[test]
fn every_fix_prints_its_pinned_text() {
    let samples = [Fix::Leftover, Fix::LiveRun];
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
