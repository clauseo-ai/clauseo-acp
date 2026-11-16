use std::collections::BTreeMap;

use super::*;

fn id(text: &str) -> AgentChoiceId {
    AgentChoiceId::new(text).unwrap()
}

fn picks(pairs: &[(&str, &str)]) -> ConfigPicks {
    ConfigPicks::new(pairs.iter().map(|(o, v)| (id(o), id(v))).collect()).unwrap()
}

#[test]
fn picks_round_trip_as_an_object() {
    let picks = picks(&[("model", "opus"), ("effort", "high")]);
    let json = serde_json::to_string(&picks).unwrap();
    assert_eq!(json, r#"{"effort":"high","model":"opus"}"#);
    assert_eq!(serde_json::from_str::<ConfigPicks>(&json).unwrap(), picks);
}

#[test]
fn more_than_the_most_picks_is_refused() {
    let map: BTreeMap<_, _> = (0..=MAX_CONFIG_PICKS)
        .map(|n| (id(&format!("o{n}")), id("v")))
        .collect();
    assert!(ConfigPicks::new(map.clone()).is_err());
    let json = serde_json::to_string(&map).unwrap();
    assert!(serde_json::from_str::<ConfigPicks>(&json).is_err());

    let mut full = ConfigPicks::new(map.into_iter().skip(1).collect()).unwrap();
    assert!(full.set(id("extra"), id("v")).is_err());
    // Replacing a pick already there adds no entry.
    full.set(id("o1"), id("w")).unwrap();
    assert_eq!(full.as_map().get(&id("o1")), Some(&id("w")));
}

#[test]
fn remove_takes_one_pick_out() {
    let mut picks = picks(&[("model", "opus"), ("effort", "high")]);
    assert!(picks.remove("effort"));
    assert!(!picks.remove("effort"));
    assert_eq!(picks, self::picks(&[("model", "opus")]));
}

#[test]
fn prefs_are_keyed_by_kind() {
    let prefs = AgentPrefs(BTreeMap::from([
        (AgentKind::Claude, picks(&[("model", "opus")])),
        (AgentKind::Codex, ConfigPicks::default()),
    ]));
    let json = serde_json::to_string(&prefs).unwrap();
    assert_eq!(json, r#"{"claude":{"model":"opus"},"codex":{}}"#);
    assert_eq!(serde_json::from_str::<AgentPrefs>(&json).unwrap(), prefs);
    assert_eq!(
        prefs.of(AgentKind::Claude),
        Some(&picks(&[("model", "opus")]))
    );
    // An empty map counts as none.
    assert_eq!(prefs.of(AgentKind::Codex), None);
    assert!(!prefs.is_empty());
    assert!(AgentPrefs::default().is_empty());
}
