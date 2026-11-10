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
        Fix::Alias => {
            "import the macro by its own name: log-literals finds warn!, error! and event! by \
             name and can't follow a rename"
        },
        Fix::Wrapper => {
            "call warn!, error! or event! directly: log-literals reads each call's message and \
             can't follow one a macro builds"
        },
    }
}

#[test]
fn every_fix_prints_its_pinned_text() {
    let samples = [Fix::Literal, Fix::Unparsable, Fix::Alias, Fix::Wrapper];
    assert_texts(&samples, pinned, ARMS);
}
