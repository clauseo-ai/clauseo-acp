//! A failed step whose text carries the guard's own command, with no refusal of the guard's
//! after it, reads [`COULD_NOT_CHECK`]; a refusal the guard gave, another failed step, and a
//! step that did not fail keep their text.

use clauseo_protocol::{TextOrBlob, ToolCallContent, ToolCallPatch, ToolCallStatus};

use super::{plain_call, plain_patch};
use crate::pre_write_guard::{COULD_NOT_CHECK, COULD_NOT_CHECK_AGENT};

const COMMAND: &str = "/Applications/Clauseo.app/Contents/MacOS/Clauseo --pre-write-guard \
                       --matter /Users/x/Matters/Smith v Jones";

fn inline(text: &str) -> TextOrBlob {
    TextOrBlob::Inline(text.to_owned())
}

/// A `failed` update as claude-agent-acp sends it: the text in a code fence as the content, the
/// same text as the raw output (a JSON string).
fn failed(text: &str) -> ToolCallPatch {
    ToolCallPatch {
        status: Some(ToolCallStatus::Failed),
        content: Some(vec![ToolCallContent::Text {
            text: inline(&format!("```\n{text}\n```")),
        }]),
        raw_output: Some(inline(&serde_json::to_string(text).unwrap())),
        ..ToolCallPatch::default()
    }
}

fn plain() -> ToolCallPatch {
    ToolCallPatch {
        status: Some(ToolCallStatus::Failed),
        content: Some(vec![ToolCallContent::Text {
            text: inline(COULD_NOT_CHECK),
        }]),
        raw_output: Some(inline(&serde_json::to_string(COULD_NOT_CHECK).unwrap())),
        ..ToolCallPatch::default()
    }
}

/// Claude Code's account of the failure, whatever its words (a timeout, another exit code with
/// stderr, a line the guard wrote itself when it caught its own error), or words a later Claude
/// Code may use: the guard's command in the brackets is what counts.
#[test]
fn a_guard_that_could_not_answer_reads_the_plain_words() {
    for (tool, rest) in [
        (
            "Write",
            "timed out; blocking because onFailure is \"block\"",
        ),
        (
            "Bash",
            "failed; blocking because onFailure is \"block\"\nthread 'main' panicked",
        ),
        ("Edit", COULD_NOT_CHECK_AGENT),
        ("Write", "No stderr output"),
        ("Write", "something a later Claude Code says"),
    ] {
        let text = format!("PreToolUse:{tool} hook error: [{COMMAND}]: {rest}");
        let mut patch = failed(&text);
        plain_patch(&mut patch);
        assert_eq!(patch, plain(), "{text}");
        // With `--ask-all` last, and with the raw output alone.
        let text = format!("PreToolUse:{tool} hook error: [{COMMAND} --ask-all]: {rest}");
        let mut patch = failed(&text);
        patch.content = None;
        plain_patch(&mut patch);
        assert_eq!(patch, plain(), "{text}");
    }
}

/// The guard's refusals keep their reason; so does a failed step without the guard's command,
/// one that names the flag outside a hook's brackets, and an update that does not fail.
#[test]
fn a_refusal_another_failure_and_an_unfailed_step_keep_their_text() {
    let kept = [
        format!(
            "PreToolUse:Write hook error: [{COMMAND}]: Drafts/paper-c.docx is open in Word or \
             another program on this computer. A change made now would be lost when that program \
             saves it. Ask the lawyer to close it, or write to a copy with another name."
        ),
        format!(
            "PreToolUse:Bash hook error: [{COMMAND}]: Clauseo can't read this command: it is \
             nested too deep or is too long. Split it into smaller commands."
        ),
        "PreToolUse:Write hook error: [/opt/firm/audit --strict]: not allowed".to_owned(),
        "Exit code 1\nerror: unknown option --pre-write-guard ]: x".to_owned(),
    ];
    for text in kept {
        let before = failed(&text);
        let mut patch = before.clone();
        plain_patch(&mut patch);
        assert_eq!(patch, before, "{text}");
    }
    let text = format!("PreToolUse:Write hook error: [{COMMAND}]: timed out");
    for status in [None, Some(ToolCallStatus::Completed)] {
        let mut before = failed(&text);
        before.status = status;
        let mut patch = before.clone();
        plain_patch(&mut patch);
        assert_eq!(patch, before);
    }
}

/// A tool call that starts `failed` gets the same words.
#[test]
fn a_call_that_starts_failed_reads_the_plain_words() {
    let text = format!("PreToolUse:Write hook error: [{COMMAND}]: timed out");
    let mut call = clauseo_acp::ToolCall {
        acp_id: "toolu_1".into(),
        title: "Write paper-c.txt".into(),
        kind: clauseo_protocol::ToolKind::Edit,
        tool_name: None,
        status: ToolCallStatus::Failed,
        locations: Vec::new(),
        raw_input: None,
        raw_output: Some(inline(&text)),
        content: Vec::new(),
    };
    plain_call(&mut call);
    let want = plain();
    assert_eq!(Some(call.content), want.content);
    assert_eq!(call.raw_output, want.raw_output);
}
