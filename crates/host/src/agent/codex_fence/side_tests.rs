use super::tests::{codex_home, data, listed, locations, notes, run_root};
use super::{Listed, SIDE_EXTENDS, SIDE_PROFILE, Session, fixed_rules, rules, side_rules};

/// The fixed rules lead [`rules`]' output, so the side profile and the session's fence never
/// drift apart.
#[test]
fn the_fixed_rules_lead_the_sessions_rules() {
    let (run_root, notes, codex_home) = (run_root(), notes(), codex_home());
    for listed in [Listed::default(), listed()] {
        let session = Session {
            run_root: &run_root,
            notes: &notes,
            codex_home: &codex_home,
            listed: &listed,
            socket_dir: None,
        };
        let all = rules(&locations(), session);
        let fixed = fixed_rules(&locations());
        assert_eq!(all[..fixed.len()], fixed[..]);
    }
}

/// The side profile: read-only, the data folder whole, then the fixed rules outside it; none on
/// Windows.
#[test]
fn the_side_rules_deny_the_data_folder() {
    assert_eq!(SIDE_PROFILE, "clauseo-side");
    assert_eq!(SIDE_EXTENDS, ":read-only");
    let side = side_rules(&locations(), false);
    let mut expected = vec![data().to_str().unwrap().to_owned()];
    for path in fixed_rules(&locations()) {
        if !path.starts_with(data()) {
            let text = path.to_str().unwrap().to_owned();
            if !expected.contains(&text) {
                expected.push(text);
            }
        }
    }
    assert_eq!(side, expected);
    assert_eq!(side_rules(&locations(), true), Vec::<String>::new());
}
