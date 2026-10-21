//! Pins the text of every `exceptions::Fix` variant with `fix_texts!`: one arm per variant, and
//! `assert_texts` fails until each arm has a sample, so a new variant needs both.

use super::Fix;
use crate::fix::tests::{assert_texts, fix_texts};

fix_texts! {
    fn pinned for Fix {
        Fix::WriteReason { .. } => {
            "write why `src/a.rs` is exempt after the path, on the same line"
        },
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
    assert_texts(&samples, pinned, ARMS);
}
