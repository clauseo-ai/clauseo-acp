//! A download Chrome holds for confirmation (`downloads/held.rs`) at the guard's level: the next
//! tool result the guard relays carries the note, once.

use serde_json::Value;

use crate::lifecycle::within;
use crate::support::{Guard, Scratch, example, guard_command, profiles};
use crate::write;

/// Whether `answer` ends with the guard's note on held downloads.
fn has_note(answer: &Value) -> bool {
    answer["result"]["content"]
        .as_array()
        .and_then(|content| content.last())
        .and_then(|block| block["text"].as_str())
        .is_some_and(|text| text.starts_with("Clauseo does not deliver executable downloads."))
}

#[test]
fn a_held_download_is_told_in_the_next_tool_result_once() {
    let scratch = Scratch::new("held_told");
    let no_args: [&std::ffi::OsStr; 0] = [];
    let mut guard = Guard::start(guard_command(&scratch, &example("daemon_stub"), &no_args));
    assert!(!has_note(&guard.call("open")));
    assert!(within(|| profiles(&scratch.cwd()).len() == 1), "no profile");
    let staging = profiles(&scratch.cwd())[0].join("downloads-staging");
    assert!(within(|| staging.is_dir()), "no staging folder");
    write::write(staging.join("Unconfirmed 123456.crdownload"), "MZ").unwrap();
    let mut calls = 0;
    assert!(
        within(|| {
            calls += 1;
            has_note(&guard.call(&format!("call-{calls}")))
        }),
        "no tool result told of the held download"
    );
    let answer = guard.call("after");
    assert!(!has_note(&answer), "told twice: {answer}");
    let (status, _) = guard.finish();
    assert_eq!(status.code(), Some(0));
}
