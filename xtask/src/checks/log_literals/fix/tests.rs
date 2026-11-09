//! Pins the text of every `log_literals::Fix` variant with `fix_texts!`: one arm per variant, and
//! `assert_texts` fails until each arm has a sample, so a new variant needs both.

use super::Fix;
use crate::fix::tests::{assert_texts, fix_texts};

fix_texts! {
    fn pinned for Fix {
        Fix::Literal => {
            "make the message a literal with no {} and put each value in a field: the runner's \
             log keeps a literal message whole and drops a string field unless its name is one \
             crates/sandbox/src/windows/runner/log_file.rs keeps"
        },
        Fix::Unparsable => {
            "make the file valid Rust: log-literals parses it to find its warn! and error! calls"
        },
    }
}

#[test]
fn every_fix_prints_its_pinned_text() {
    let samples = [Fix::Literal, Fix::Unparsable];
    assert_texts(&samples, pinned, ARMS);
}
