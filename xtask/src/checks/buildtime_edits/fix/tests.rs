//! Pins the text of every `buildtime_edits::Fix` variant with `fix_texts!`: one arm per variant,
//! and `assert_texts` fails until each arm has a sample, so a new variant needs both.

use super::Fix;
use crate::fix::tests::{assert_texts, fix_texts};

fix_texts! {
    fn pinned for Fix {
        Fix::Leftover => {
            "run `cargo xtask buildtime web` once to restore the file from target/xtask/buildtime, \
             or remove the attribute by hand"
        },
        Fix::LiveRun => "rerun the check when that run ends",
    }
}

#[test]
fn every_fix_prints_its_pinned_text() {
    let samples = [Fix::Leftover, Fix::LiveRun];
    assert_texts(&samples, pinned, ARMS);
}
