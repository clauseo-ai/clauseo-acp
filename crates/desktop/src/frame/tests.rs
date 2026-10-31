use clauseo_protocol::{
    AccountLimits, AccountRef, AgentKind, AgentTextEntry, AppendTarget, AtLimitBehaviour, Billing,
    Cursor, ErrorCode, HostError, HostEvent, LimitSource, MessageId, PatchBatch, Seq, StreamId,
    TextKind, ThreadId, Timestamp, TurnId, ViewEntry, ViewEpoch, ViewPatch, ViewPatchBody,
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
fn reassemble(messages: &[Frame], id: u32) -> String {
    if let [only] = messages {
        return only.text().to_owned();
    }
    let mut joined = String::new();
    for (index, sent) in messages.iter().enumerate() {
        let message: serde_json::Value = serde_json::from_str(sent.text()).unwrap();
        let fragment = &message["fragment"];
        assert_eq!(fragment["id"], id);
        assert_eq!(fragment["index"], index);
        assert_eq!(fragment["count"], messages.len());
        joined.push_str(fragment["text"].as_str().unwrap());
    }
    joined
}

fn assert_framed(json: &str, id: u32) {
    let messages = messages(json, id).unwrap();
    assert!(messages.len() > 1, "a {} byte event went whole", json.len());
    for message in &messages {
        assert!(
            message.len() < FRAME_LIMIT,
            "a message of {} bytes",
            message.len()
        );
    }
    let joined = reassemble(&messages, id);
    assert_eq!(joined, json);
    let _: HostEvent = serde_json::from_str(&joined).unwrap();
}

#[test]
fn no_message_reaches_8_kb_for_a_100_kb_append_text_or_a_20_kb_upsert() {
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
    let messages = messages(&json, 0).unwrap();
    assert_eq!(messages, vec![Frame(json)]);
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
    assert_eq!(messages(&below, 1).unwrap().len(), 1);
}

#[test]
fn a_raw_control_character_is_escaped_inside_a_fragment() {
    let json = format!("\"{}\u{1}\"", "x".repeat(FRAME_LIMIT));
    let messages = messages(&json, 2).unwrap();
    assert!(messages.iter().all(|message| message.len() < FRAME_LIMIT));
    assert_eq!(reassemble(&messages, 2), json);
}

/// The top-level keys the reader takes for a message of the bridge's own, never an event's.
const RESERVED: [&str; 2] = ["fragment", "stream_failed"];

/// The number of `HostEvent` variants [`variant`] names.
const VARIANTS: usize = 4;

/// Each `HostEvent` variant's place. A new variant fails to compile here, so it gets a place, a
/// sample in the test below, and its key checked against [`RESERVED`].
fn variant(event: &HostEvent) -> usize {
    match event {
        HostEvent::Patches(_) => 0,
        HostEvent::Events(_) => 1,
        HostEvent::CaughtUp { .. } => 2,
        HostEvent::AccountLimitsChanged(_) => 3,
    }
}

#[test]
fn no_host_event_kind_takes_a_reserved_key() {
    let stream = StreamId::Thread(ThreadId::new(v7()).unwrap());
    let samples = [
        HostEvent::Patches(PatchBatch {
            stream,
            epoch: ViewEpoch::new(v7()).unwrap(),
            patches: Vec::new(),
        }),
        HostEvent::Events(Vec::new()),
        HostEvent::CaughtUp {
            stream,
            last_seq: Seq::new(1).unwrap(),
        },
        HostEvent::AccountLimitsChanged(AccountLimits {
            agent: AgentKind::Claude,
            account: AccountRef("acct-1".to_owned()),
            plan: None,
            billing: Billing::Plan,
            windows: Vec::new(),
            at_limit: AtLimitBehaviour::Pauses,
            source: LimitSource::Fresh {
                checked_at: Timestamp::new(1_760_000_000_000).unwrap(),
            },
        }),
    ];
    let mut places: Vec<usize> = samples.iter().map(variant).collect();
    places.sort_unstable();
    assert_eq!(places, (0..VARIANTS).collect::<Vec<_>>());
    for sample in &samples {
        let value = serde_json::to_value(sample).unwrap();
        let keys: Vec<&String> = value.as_object().unwrap().keys().collect();
        assert_eq!(keys.len(), 1, "{value}");
        assert!(!RESERVED.contains(&keys[0].as_str()), "{value}");
    }
}

#[test]
fn a_stream_failure_is_one_message_below_the_limit() {
    let stream = StreamId::Thread(ThreadId::new(v7()).unwrap());
    let error = HostError::new(ErrorCode::StoreIo, "disk full");
    let message = stream_failed(stream, &error);
    let value: serde_json::Value = serde_json::from_str(message.text()).unwrap();
    assert_eq!(
        value,
        serde_json::json!({ "stream_failed": { "stream": stream, "error": error } })
    );
    // A message long enough to pass the limit goes with its code and a fixed message.
    let long = HostError::new(ErrorCode::StoreIo, "\u{1}".repeat(FRAME_LIMIT));
    let message = stream_failed(stream, &long);
    assert!(message.len() < FRAME_LIMIT);
    let value: serde_json::Value = serde_json::from_str(message.text()).unwrap();
    assert_eq!(value["stream_failed"]["error"]["code"], "store.io");
}
