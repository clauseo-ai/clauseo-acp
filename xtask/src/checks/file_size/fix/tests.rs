//! Pins the text of every `file_size::Fix` variant. The `match` has no `_` arm, so a new variant
//! does not compile until it has a pinned text here.

use super::Fix;
use std::collections::HashSet;
use std::mem::discriminant;

fn pinned(fix: &Fix) -> &'static str {
    match fix {
        Fix::SplitOrExempt => {
            "split it into smaller modules, or list it in xtask/file-size-exceptions.txt with a \
             reason"
        }
        Fix::WrapComment => "wrap the comment at 100 columns",
        Fix::DeleteException => "delete this line from xtask/file-size-exceptions.txt",
    }
}

#[test]
fn every_fix_prints_its_pinned_text() {
    let samples = [Fix::SplitOrExempt, Fix::WrapComment, Fix::DeleteException];
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
