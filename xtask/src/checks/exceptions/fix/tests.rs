//! Pins the text of every `exceptions::Fix` variant. The `match` has no `_` arm, so a new variant
//! does not compile until it has a pinned text here.

use super::Fix;
use std::collections::HashSet;
use std::mem::discriminant;

fn pinned(fix: &Fix) -> &'static str {
    match fix {
        Fix::WriteReason { .. } => {
            "write why `src/a.rs` is exempt after the path, on the same line"
        }
        Fix::DeleteDuplicate => "delete this line and keep the reason on the first one",
    }
}

#[test]
fn every_fix_prints_its_pinned_text() {
    let samples = [
        Fix::WriteReason {
            path: "src/a.rs".to_owned(),
        },
        Fix::DeleteDuplicate,
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
