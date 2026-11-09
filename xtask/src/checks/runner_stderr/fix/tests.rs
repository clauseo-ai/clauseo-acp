//! Pins the text of every `runner_stderr::Fix` variant with `fix_texts!`: one arm per variant, and
//! `assert_texts` fails until each arm has a sample, so a new variant needs both.

use super::Fix;
use crate::fix::tests::{assert_texts, fix_texts};

fix_texts! {
    fn pinned for Fix {
        Fix::Console => {
            "say the line through runner::stderr with stderr::refuse or stderr::notice: the host \
             copies the runner's stderr into the matter's event log, so only the API's fixed \
             sentences may go to it"
        },
        Fix::WriteStderr => {
            "say the line through runner::stderr with stderr::refuse or stderr::notice rather than \
             writing to a stderr handle: runner-stderr fails write! and writeln! whose target is \
             stderr"
        },
        Fix::StderrCall => {
            "take the stream from runner::stderr (stderr::refuse, stderr::notice or \
             stderr::agent_stream) instead of opening stderr: runner-stderr fails each call that \
             opens it"
        },
        Fix::ErrorText => {
            "format the error inside a Detail::new(|| ...) argument, which only the \
             vm-harness-detail builds call: error text can hold a path, and the host copies the \
             runner's stderr into the event log"
        },
        Fix::Alias => {
            "import Detail and stderr under their own names: runner-stderr finds them by name and \
             can't follow a rename"
        },
        Fix::Wrapper => {
            "write the console or stderr call where it is needed, not in a macro_rules! body: \
             runner-stderr reads the calls it can see and can't follow one a macro builds"
        },
        Fix::Unparsable => {
            "make the file valid Rust: runner-stderr parses it to find its stderr and format calls"
        },
    }
}

#[test]
fn every_fix_prints_its_pinned_text() {
    let samples = [
        Fix::Console,
        Fix::WriteStderr,
        Fix::StderrCall,
        Fix::ErrorText,
        Fix::Alias,
        Fix::Wrapper,
        Fix::Unparsable,
    ];
    assert_texts(&samples, pinned, ARMS);
}
