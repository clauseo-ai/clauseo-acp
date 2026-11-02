use super::*;

#[test]
fn the_file_has_the_brief_s_shape() {
    let mut state = State::default();
    state.add("case-law", CredentialKind::OAuth);
    assert_eq!(
        String::from_utf8(state.to_bytes()).unwrap(),
        "{\"v\":1,\"connectors\":{\"case-law\":{\"kind\":\"oauth\",\"ended\":null}}}\n"
    );
    state.end("case-law", Ended::KeyRevoked);
    assert!(
        String::from_utf8(state.to_bytes())
            .unwrap()
            .contains(r#""ended":"key_revoked""#)
    );
    state.end("case-law", Ended::SignedOut);
    assert!(
        String::from_utf8(state.to_bytes())
            .unwrap()
            .contains(r#""ended":"signed_out""#)
    );
    assert_eq!(State::parse(&state.to_bytes()), state);
}

#[test]
fn status_follows_the_entry_and_the_item() {
    let mut state = State::default();
    assert_eq!(state.status("case-law", None), CredentialStatus::NotAdded);
    assert_eq!(
        state.status("case-law", Some(CredentialKind::OAuth)),
        CredentialStatus::NotAdded,
        "an item with no entry is not added"
    );
    state.add("case-law", CredentialKind::ApiKey);
    assert_eq!(
        state.status("case-law", Some(CredentialKind::ApiKey)),
        CredentialStatus::SignedIn(CredentialKind::ApiKey)
    );
    assert_eq!(state.status("case-law", None), CredentialStatus::SignedOut);
    state.end("case-law", Ended::KeyRevoked);
    assert_eq!(state.status("case-law", None), CredentialStatus::KeyRevoked);
    state.remove("case-law");
    assert_eq!(state.status("case-law", None), CredentialStatus::NotAdded);
}

#[test]
fn a_bad_file_reads_as_empty_and_a_bad_entry_is_left_out() {
    assert_eq!(State::parse(b"not json"), State::default());
    assert_eq!(State::parse(b"[]"), State::default());
    let state = State::parse(
        br#"{"v":1,"later":true,"connectors":{"case-law":{"kind":"api_key","ended":"key_revoked","x":1},"other":{"kind":"magic"}}}"#,
    );
    assert_eq!(state.connectors.len(), 1);
    assert_eq!(
        state.connectors["case-law"],
        Entry {
            kind: CredentialKind::ApiKey,
            ended: Some(Ended::KeyRevoked)
        }
    );
}

#[cfg(feature = "test-support")]
#[test]
fn save_and_load_round_trip_through_the_disk() {
    let scratch = crate::write::ScratchDir::new("state").unwrap();
    let path = scratch.state_file();
    assert_eq!(
        State::load(&path).unwrap(),
        State::default(),
        "missing is empty"
    );
    let mut state = State::default();
    state.add("case-law", CredentialKind::OAuth);
    state.save(&path).unwrap();
    assert_eq!(State::load(&path).unwrap(), state);
    state.end("case-law", Ended::SignedOut);
    state.save(&path).unwrap();
    assert_eq!(State::load(&path).unwrap(), state);
    let names: Vec<_> = std::fs::read_dir(path.parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names, ["state.json"], "no temp file is left");
}
