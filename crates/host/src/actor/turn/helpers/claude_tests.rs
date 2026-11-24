//! Claude's launch input, handback and end reading (`claude.rs`), on the frames of the recorded
//! run (issue 13, fixture S1, paths moved under /Users/lawyer).

use clauseo_protocol::{ContentBlock, TextOrBlob, ToolCallContent, ToolCallPatch, ToolCallStatus};

use super::{Input, defined_model, delivered, input_of, is_handback, is_launch, text_of};

fn inline(text: &str) -> Option<TextOrBlob> {
    Some(TextOrBlob::Inline(text.to_owned()))
}

fn text_content(text: &str) -> Vec<ToolCallContent> {
    vec![ToolCallContent::Content {
        content: ContentBlock::Text {
            text: text.to_owned(),
        },
    }]
}

#[test]
fn the_agent_and_task_tools_launch_and_the_handback_ends() {
    assert!(is_launch(Some("Agent")));
    assert!(is_launch(Some("Task")));
    assert!(!is_launch(Some("Read")));
    assert!(!is_launch(None));
    assert!(is_handback(Some("SubagentHandback")));
    assert!(!is_handback(Some("Agent")));
}

/// S1.jsonl line 33's `rawInput`.
#[test]
fn a_launch_input_gives_the_name_label_and_task() {
    let raw = r#"{"description":"Digest lease and notice","subagent_type":"paper-reader","prompt":"Read the files lease.txt and notice.txt in the working directory /Users/lawyer/matter and return one short digest covering both documents."}"#;
    let input = input_of(inline(raw).as_ref()).unwrap();
    assert_eq!(input.name.as_deref(), Some("paper-reader"));
    assert_eq!(input.label.as_deref(), Some("Digest lease and notice"));
    assert!(
        input
            .task
            .as_deref()
            .unwrap()
            .starts_with("Read the files lease.txt")
    );
    assert_eq!(input.model, None);
    assert!(!input.background);
}

#[test]
fn a_launch_input_without_a_type_takes_its_name_and_model() {
    let raw =
        r#"{"name":"checker","prompt":"Check it.","model":"sonnet","run_in_background":true}"#;
    let input = input_of(inline(raw).as_ref()).unwrap();
    assert_eq!(
        input,
        Input {
            name: Some("checker".into()),
            label: None,
            task: Some("Check it.".to_owned()),
            model: Some("sonnet".into()),
            background: true,
        }
    );
}

#[test]
fn an_input_that_names_no_helper_nor_task_is_not_a_launch_input() {
    assert_eq!(input_of(inline("{}").as_ref()), None);
    assert_eq!(input_of(inline(r#"{"description":"x"}"#).as_ref()), None);
    assert_eq!(input_of(inline("not json").as_ref()), None);
    assert_eq!(input_of(None), None);
}

#[test]
fn the_profiles_helpers_have_their_model() {
    let model = defined_model(clauseo_profile::subagents::PAPER_READER);
    assert_eq!(
        model.as_deref(),
        Some(clauseo_profile::subagents::PAPER_READER_MODEL)
    );
    assert_eq!(defined_model("no-such-helper"), None);
}

/// S1.jsonl line 60: the handback's update says the report landed.
#[test]
fn a_handback_lands_unless_it_says_success_false() {
    let landed = ToolCallPatch {
        status: Some(ToolCallStatus::Completed),
        content: Some(text_content(
            r#"{"success":true,"message":"Report delivered to your caller."}"#,
        )),
        ..ToolCallPatch::default()
    };
    assert!(delivered(&landed));
    let refused = ToolCallPatch {
        content: Some(text_content(
            r#"{"success":false,"message":"Nothing was sent."}"#,
        )),
        ..landed.clone()
    };
    assert!(!delivered(&refused));
    let unread = ToolCallPatch {
        content: Some(text_content("garbled")),
        ..landed.clone()
    };
    assert!(delivered(&unread));
    let empty = ToolCallPatch {
        content: None,
        ..landed
    };
    assert!(delivered(&empty));
}

#[test]
fn a_steps_text_joins_its_text_items() {
    let mut content = text_content("One.");
    content.push(ToolCallContent::Text {
        text: TextOrBlob::Inline("Two.".to_owned()),
    });
    assert_eq!(text_of(&content).as_deref(), Some("One.\n\nTwo."));
    assert_eq!(text_of(&[]), None);
}
