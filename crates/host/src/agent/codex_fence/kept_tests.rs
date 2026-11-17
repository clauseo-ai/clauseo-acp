//! The glob that denies Codex every matter's kept Claude rules file (`KEPT_RULES_GLOB`).

use serde_json::json;

use super::tests::{data, fence_with, notes};
use super::{KEPT_RULES_GLOB, Listed, PROFILE};

/// A matter's kept Claude rules file made after a Codex launch (by a Claude session of the same
/// matter) is in no listed rule, since the launch listed only what existed then; the glob denies
/// it, and every other matter's, whatever the listing found.
#[test]
fn a_kept_rules_file_made_after_the_launch_is_denied_by_the_glob() {
    let own = data().join("agent-state").join("m1");
    // The launch found this matter's folder holding only its notes, and matter m2's folder.
    let listed = Listed {
        agent_state: Some(vec![data().join("agent-state").join("m2")]),
        ..Listed::default()
    };
    let keys = fence_with(&listed);
    let scoped = keys["permissions"][PROFILE]["filesystem"][data().to_str().unwrap()]
        .as_object()
        .expect("rules under <data>");
    assert!(
        !scoped.contains_key("agent-state/m1/claude-allow.json"),
        "the listing knew no file"
    );
    assert_eq!(scoped.get(KEPT_RULES_GLOB), Some(&json!("deny")));
    // The glob names exactly the file the host keeps, for this matter and any other.
    for matter in ["m1", "m2", "m9"] {
        let relative = format!("agent-state/{matter}/{}", crate::agent::always::FILE);
        assert_eq!(KEPT_RULES_GLOB.replace('*', matter), relative);
    }
    assert_eq!(
        own.join(crate::agent::always::FILE),
        crate::agent::always::file_beside(&notes()).unwrap()
    );
}
