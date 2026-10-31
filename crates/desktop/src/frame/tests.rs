use clauseo_protocol::{
    AgentTextEntry, AppendTarget, Cursor, HostEvent, MessageId, PatchBatch, Seq, StreamId,
    TextKind, ThreadId, TurnId, ViewEntry, ViewEpoch, ViewPatch, ViewPatchBody,
};
use uuid::Uuid;

use super::*;

/// Text that is costly to escape: quotes, backslashes, a newline, a tab, and characters of two,
/// three and four bytes.
const AWKWARD: &str = "He said \"no\" \\ then\n\tleft: é € 𝄞 ";

fn v7() -> Uuid {
    Uuid::now_v7()
}

/// `len` bytes or more of awkward text.
fn text(len: usize) -> String {
    AWKWARD.repeat(len / AWKWARD.len() + 1)
}

fn batch(patches: Vec<ViewPatchBody>) -> String {
    let event = HostEvent::Patches(PatchBatch {
        stream: StreamId::Thread(ThreadId::new(v7()).unwrap()),
        epoch: ViewEpoch::new(v7()).unwrap(),
        patches: (1..)
            .zip(patches)
            .map(|(cursor, body)| ViewPatch {
                cursor: Cursor::new(cursor).unwrap(),
                body,
            })
            .collect(),
    });
    serde_json::to_string(&event).unwrap()
}

/// What the transport does: join the fragments' texts in index order, or take the event as it is.
fn reassemble(frames: &[Frame], id: u32) -> String {
    if let [only] = frames {
        return only.text().to_owned();
    }
    let mut joined = String::new();
    for (index, frame) in frames.iter().enumerate() {
        let message: serde_json::Value = serde_json::from_str(frame.text()).unwrap();
        let fragment = &message["fragment"];
        assert_eq!(fragment["id"], id);
        assert_eq!(fragment["index"], index);
        assert_eq!(fragment["count"], frames.len());
        joined.push_str(fragment["text"].as_str().unwrap());
    }
    joined
}

fn assert_framed(json: &str, id: u32) {
    let frames = frames(json, id).unwrap();
    assert!(frames.len() > 1, "a {} byte event went whole", json.len());
    for frame in &frames {
        assert!(
            frame.len() < FRAME_LIMIT,
            "a frame of {} bytes",
            frame.len()
        );
    }
    let joined = reassemble(&frames, id);
    assert_eq!(joined, json);
    let _: HostEvent = serde_json::from_str(&joined).unwrap();
}

#[test]
fn no_frame_reaches_8_kb_for_a_100_kb_append_text_or_a_20_kb_upsert() {
    let message = MessageId::new(v7()).unwrap();
    let appends = (0..25)
        .map(|i| ViewPatchBody::AppendText {
            message_id: message,
            target: AppendTarget::AgentText,
            from_len: i * 4000,
            text: text(4000),
        })
        .collect();
    let appends = batch(appends);
    assert!(appends.len() > 100_000);
    assert_framed(&appends, 7);

    let upsert = ViewPatchBody::Upsert {
        entry: Box::new(ViewEntry::AgentText(AgentTextEntry {
            order: Seq::new(3).unwrap(),
            message,
            turn: TurnId::new(v7()).unwrap(),
            kind: TextKind::Answer,
            text: text(20_000),
        })),
    };
    let upsert = batch(vec![upsert]);
    assert!(upsert.len() > 20_000);
    assert_framed(&upsert, u32::MAX);
}

#[test]
fn an_event_below_the_limit_goes_as_it_is() {
    let json = batch(vec![ViewPatchBody::AppendText {
        message_id: MessageId::new(v7()).unwrap(),
        target: AppendTarget::AgentText,
        from_len: 0,
        text: text(100),
    }]);
    let frames = frames(&json, 0).unwrap();
    assert_eq!(frames, vec![Frame(json)]);
}

#[test]
fn the_limit_is_strict_at_8192_bytes() {
    // An event of exactly 8192 bytes is fragmented: Tauri's direct path needs fewer.
    let mut json = batch(vec![ViewPatchBody::AppendText {
        message_id: MessageId::new(v7()).unwrap(),
        target: AppendTarget::AgentText,
        from_len: 0,
        text: String::new(),
    }]);
    let pad = FRAME_LIMIT - json.len();
    json = json.replacen(
        r#""text":"""#,
        &format!(r#""text":"{}""#, "a".repeat(pad)),
        1,
    );
    assert_eq!(json.len(), FRAME_LIMIT);
    assert_framed(&json, 1);
    let below = json.replacen("aa", "a", 1);
    assert_eq!(frames(&below, 1).unwrap().len(), 1);
}

#[test]
fn a_raw_control_character_is_escaped_inside_a_fragment() {
    let json = format!("\"{}\u{1}\"", "x".repeat(FRAME_LIMIT));
    let frames = frames(&json, 2).unwrap();
    assert!(frames.iter().all(|frame| frame.len() < FRAME_LIMIT));
    assert_eq!(reassemble(&frames, 2), json);
}
