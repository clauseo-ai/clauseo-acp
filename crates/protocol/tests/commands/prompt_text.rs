//! The prompt text limit of `SendPrompt` and `EditQueuedPrompt`: 6 MiB counted as JSON-escaped
//! bytes (`escaped_len`) summed over the `Text` blocks, and its own code, `prompt.too_long`.
//! Shared helpers are in `commands.rs`.

use clauseo_protocol::{
    Error, ErrorCode, HostError, MAX_PROMPT_TEXT_BYTES, Measure, PromptBlock, ValidatedCommand,
    escaped_len,
};

use super::fixtures::file_ref_id;
use super::{Validate, edit_queued, send_prompt, text, too_large};

const MAX: usize = MAX_PROMPT_TEXT_BYTES;

/// Whether `result` is the prompt text measure over the limit by exactly `size`, and reaches
/// the UI as `prompt.too_long`.
fn too_long(result: Result<ValidatedCommand, Error>, size: usize) -> bool {
    if !too_large(&result, size, MAX) {
        return false;
    }
    let Err(error) = result else { return false };
    let typed = matches!(
        error,
        Error::TooLarge {
            what: Measure::PromptText,
            ..
        }
    );
    let host = HostError::invalid_command(error);
    typed && host.code() == ErrorCode::PromptTooLong && !host.retryable()
}

fn send(blocks: Vec<PromptBlock>) -> Result<ValidatedCommand, Error> {
    send_prompt(blocks).validate()
}

/// Plain ASCII: escaping adds nothing, so the cap is the raw length. At the limit passes, one
/// byte over fails, summed across blocks, a file block between them changing nothing.
#[test]
fn plain_text_at_the_limit_passes_and_one_over_fails() {
    let max_text = "a".repeat(MAX);
    let half = "b".repeat(MAX / 2);
    assert!(send(vec![text(&max_text)]).is_ok());
    assert!(send(vec![text(&half), text(&half)]).is_ok());
    assert!(too_long(send(vec![text(&(max_text + "x"))]), MAX + 1));
    let file = PromptBlock::File {
        file: file_ref_id(100),
    };
    let split = vec![text(&half), file, text(&(half.clone() + "x"))];
    assert!(too_long(send(split), MAX + 1));
}

/// Where the escaped length differs from the raw one: `\n` and `"` cost 2 bytes, U+0001 costs 6
/// (`\u0001`). Each passes at the escaped limit, a sixth or half of the raw bytes, and fails one
/// escaped byte over. Non-ASCII is not escaped: `é` costs its 2 UTF-8 bytes.
#[test]
fn the_cap_counts_escaped_bytes() {
    for (unit, cost) in [("\n", 2), ("\"", 2), ("\u{1}", 6), ("é", 2)] {
        assert_eq!(escaped_len(unit), cost, "{unit:?}");
        assert_eq!(MAX % cost, 0, "the limit is a whole number of {unit:?}");
        // Led by a visible character, as a prompt of only line breaks is refused.
        let at_limit = "a".repeat(cost) + &unit.repeat(MAX / cost - 1);
        assert!(send(vec![text(&at_limit)]).is_ok(), "{unit:?} at the limit");
        let over = format!("{at_limit}a");
        assert!(
            too_long(send(vec![text(&over)]), MAX + 1),
            "{unit:?} one over"
        );
        assert!(
            too_long(edit_queued(&over).validate(), MAX + 1),
            "{unit:?} edit one over"
        );
        assert!(
            edit_queued(&at_limit).validate().is_ok(),
            "{unit:?} edit at the limit"
        );
    }
    // A raw cap would pass this: 1 MiB of raw bytes that escape to 6 MiB plus 6.
    let control = "\u{1}".repeat(MAX / 6 + 1);
    assert!(control.len() < MAX);
    assert!(too_long(send(vec![text(&control)]), MAX + 6));
}

/// `EditQueuedPrompt` carries text only (its file blocks stay with the queued turn): the same
/// limit and code apply, and empty text passes the shape check, since the host knows the files.
#[test]
fn edit_queued_text_limit() {
    let max_text = "a".repeat(MAX);
    assert!(edit_queued(&max_text).validate().is_ok());
    assert!(edit_queued("").validate().is_ok());
    assert!(too_long(edit_queued(&(max_text + "x")).validate(), MAX + 1));
}
