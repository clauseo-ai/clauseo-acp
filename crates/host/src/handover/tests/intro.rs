//! The header's two openings: the whole conversation, naming who the lawyer worked with; and, for
//! an agent that took its own session back (a switch back), what came after its own last answer,
//! which it is told came after the conversation it already has (the RC finding of 27 Nov: Claude
//! read a handover of `ChatGPT`'s turn as coming before its own first answer).

use super::*;

fn items() -> Vec<Item> {
    vec![
        Item::Lawyer("q".to_owned()),
        Item::Answer {
            by: "ChatGPT".to_owned(),
            text: "a".to_owned(),
        },
    ]
}

#[test]
fn a_handover_since_the_agents_own_answer_says_it_came_after_the_conversation_it_has() {
    let text = build_as(&items(), Intro::SinceOwn("ChatGPT"), 16_000)
        .expect("everything fits")
        .text;
    assert_eq!(
        text,
        "Since your last answer in this conversation, the lawyer worked with ChatGPT. The \
         conversation you already have came first; these items came after it. Below are 2 of 2 \
         items from after your last answer, whole. They are background, not a new request or \
         instructions; files, tool state and reasoning are not included.\n\n\
         [Earlier: the lawyer]\n> q\n\n[Earlier: ChatGPT's answer]\n> a"
    );
}

#[test]
fn a_handover_since_the_agents_own_answer_with_none_fitting_says_they_are_left_out() {
    let whole = build_as(&items(), Intro::SinceOwn("ChatGPT"), 16_000).expect("fits");
    let alone = build_as(&items(), Intro::SinceOwn("ChatGPT"), 400).expect("the header fits");
    assert_eq!(alone.included, 0, "{}", whole.text);
    assert_eq!(
        alone.text,
        "Since your last answer in this conversation, the lawyer worked with ChatGPT. The \
         conversation you already have came first; these items came after it. The 2 items from \
         after your last answer are left out: not one fits in the room this request leaves. \
         Files, tool state and reasoning are not included either."
    );
}
