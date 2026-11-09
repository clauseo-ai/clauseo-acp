use super::capped;

/// A mode id in a log line is cut at 64 characters, with an ellipsis; a shorter one is as it came.
#[test]
fn a_long_mode_id_is_cut_in_the_log() {
    assert_eq!(capped("agent-full-access"), "agent-full-access");
    let exact = "m".repeat(64);
    assert_eq!(capped(&exact), exact);
    let long = "é".repeat(100);
    assert_eq!(capped(&long), format!("{}…", "é".repeat(64)));
}
