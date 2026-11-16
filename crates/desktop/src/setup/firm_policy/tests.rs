//! The `firm-policy` marker: written while the policy sets an agent's endpoint, and with the
//! policy unreadable, the agent with a marker is refused and one without is not.

use clauseo_protocol::AgentId;
use clauseo_settings::{Effective, FilePolicy, Locations, PolicySource, SystemFiles};

use super::{firm_policy, marker};
use crate::test_support::Scratch;

fn claude() -> AgentId {
    AgentId::new("claude-acp").unwrap()
}

fn codex() -> AgentId {
    AgentId::new("codex-acp").unwrap()
}

/// The effective settings under `scratch` with `policy.json` holding `policy`.
fn effective(scratch: &Scratch, policy: &[u8]) -> Effective {
    let locations = Locations::under_root(scratch.path());
    let path = scratch.file("policy.json", policy);
    let source = FilePolicy::new(path, SystemFiles);
    let sources: Vec<&dyn PolicySource> = vec![&source];
    Effective::load(&locations, &SystemFiles, &sources)
}

#[test]
fn the_marker_is_written_for_a_policy_endpoint_and_refuses_sign_in_while_policy_is_unreadable() {
    let scratch = Scratch::folder("firm-policy", &[]);
    let set = effective(
        &scratch,
        br#"{"plans": {"claude-acp": {"endpoint": "https://gateway.firm.example"}}}"#,
    );
    assert!(set.policy().errors().is_empty(), "{:?}", set.policy());
    let blocked = firm_policy(&set, &SystemFiles, [&claude(), &codex()]);
    assert!(blocked.is_empty(), "a readable policy refuses nothing");
    let data_dir = set.data_dir().to_path_buf();
    assert!(
        marker(&data_dir, &claude()).is_file(),
        "the marker is written"
    );
    assert!(!marker(&data_dir, &codex()).exists(), "no plan, no marker");

    let unreadable = effective(&scratch, b"{ not json");
    assert!(!unreadable.policy().errors().is_empty());
    let blocked = firm_policy(&unreadable, &SystemFiles, [&claude(), &codex()]);
    assert_eq!(blocked.into_iter().collect::<Vec<_>>(), [claude()]);
}
