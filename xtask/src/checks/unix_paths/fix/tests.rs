//! Pins the text of every `unix_paths::Fix` variant with `fix_texts!`: one arm per variant, and
//! `assert_texts` fails until each arm has a sample, so a new variant needs both.

use super::Fix;
use crate::fix::tests::{assert_texts, fix_texts};

fix_texts! {
    fn pinned for Fix {
        Fix::UseAbs => {
            "pass it through a helper that makes it absolute on every OS, such as the host's \
             `crate::testing::abs(..)`; or keep the code off Windows with `#[cfg(unix)]`; or, when \
             the path never reaches a path API, write `// unix-paths-ok: <reason>` on the line above"
        },
        Fix::Stale { .. } => "delete line 4 of xtask/unix-paths-baseline.txt",
        Fix::Malformed => {
            "write the line as the repo-relative path, a space and the literal in double quotes, as \
             the comment at the top of xtask/unix-paths-baseline.txt says, or delete it"
        },
        Fix::GiveReason => {
            "write why the path may stay Unix-only after `// unix-paths-ok:`, or delete the marker"
        },
        Fix::NotLexed => {
            "make the file valid Rust tokens: unix-paths lexes every test file for its string \
             literals"
        },
    }
}

#[test]
fn every_fix_prints_its_pinned_text() {
    let samples = [
        Fix::UseAbs,
        Fix::Stale { line: 4 },
        Fix::Malformed,
        Fix::GiveReason,
        Fix::NotLexed,
    ];
    assert_texts(&samples, pinned, ARMS);
}
