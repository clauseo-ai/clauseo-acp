//! `SiteBlocked`'s wire form: no thread or turn leaves the keys out, and a tag from a newer
//! Clauseo decodes to `SiteTag::Unknown` instead of failing the event.

use clauseo_protocol::{HostEvent, SiteTag, Timestamp};

use super::fixtures::{blocked_site_id, matter_id, round_trips};

fn blocked(tag: SiteTag) -> HostEvent {
    HostEvent::SiteBlocked {
        block_id: blocked_site_id(1),
        matter: matter_id(2),
        thread: None,
        turn: None,
        host: "1.2.3.4".to_owned(),
        tag,
        list_generated: Timestamp::new(1_794_031_200_000).unwrap(),
        override_allowed: false,
        count: 1,
    }
}

#[test]
fn site_blocked_without_a_turn_leaves_out_thread_and_turn() {
    let event = blocked(SiteTag::Malware);
    let value = serde_json::to_value(&event).unwrap();
    let body = &value["site_blocked"];
    assert!(body.get("thread").is_none(), "{value}");
    assert!(body.get("turn").is_none(), "{value}");
    assert_eq!(body["tag"], serde_json::json!("malware"));
    round_trips(&event);
}

#[test]
fn a_newer_tag_decodes_to_unknown() {
    let mut value = serde_json::to_value(blocked(SiteTag::Phishing)).unwrap();
    value["site_blocked"]["tag"] = serde_json::json!("cryptojacking");
    let event: HostEvent = serde_json::from_value(value).unwrap();
    assert_eq!(event, blocked(SiteTag::Unknown));
}
