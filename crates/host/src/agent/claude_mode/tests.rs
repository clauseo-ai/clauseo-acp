use super::*;

/// Every launch kind and the mode it asks for: only a jailed launch that is not forced ask runs
/// in auto. A launch's kind is all there is: a restart and a `session/load` are launches too, so
/// a thread loaded once jailed and once unjailed gets each row's mode (`tests/host/
/// claude_auto.rs` drives them through the host). R17 PR-D2f's switched-off case adds its row.
#[test]
fn only_a_jailed_launch_that_is_not_forced_ask_runs_in_auto() {
    let table: [(&str, bool, bool, &str); 4] = [
        ("unjailed (tier C)", false, false, "default"),
        ("unjailed, forced ask", false, true, "default"),
        ("jailed, forced ask", true, true, "default"),
        ("jailed (tiers A and B)", true, false, "auto"),
    ];
    for (kind, jailed, forced_ask, mode) in table {
        assert_eq!(launch_mode(jailed, forced_ask), mode, "{kind}");
        if !jailed {
            assert_ne!(launch_mode(jailed, forced_ask), AUTO, "{kind}");
            assert_ne!(launch_mode(jailed, forced_ask), UNAVAILABLE, "{kind}");
        }
    }
}

fn rpc(details: Option<&str>) -> clauseo_acp::Error {
    clauseo_acp::Error::Rpc {
        code: -32603,
        message: "Internal error".to_owned(),
        details: details.map(str::to_owned),
    }
}

#[test]
fn a_setting_that_turned_auto_off_falls_back_to_asking_and_anything_else_to_accept_edits() {
    let turned_off = "Cannot set permission mode to auto: auto mode disabled by settings";
    assert_eq!(
        Fallback::of_refusal(&rpc(Some(turned_off))),
        Fallback::TurnedOff
    );
    for other in [
        Some("Cannot set permission mode to auto: auto mode is unavailable for your plan"),
        Some("Cannot set permission mode to auto: auto mode unavailable for this model"),
        Some("Cannot transition to auto mode: gate is not enabled"),
        None,
    ] {
        assert_eq!(
            Fallback::of_refusal(&rpc(other)),
            Fallback::Unavailable,
            "{other:?}"
        );
    }
    assert_eq!(
        Fallback::of_refusal(&clauseo_acp::Error::Closed),
        Fallback::Unavailable
    );
    assert_eq!(Fallback::TurnedOff.mode(), "default");
    assert_eq!(Fallback::Unavailable.mode(), "acceptEdits");
}

#[test]
fn the_mode_a_jailed_session_runs_in_names_its_fallback() {
    assert_eq!(Fallback::of_mode("auto"), None);
    assert_eq!(Fallback::of_mode("plan"), None);
    assert_eq!(
        Fallback::of_mode("acceptEdits"),
        Some(Fallback::Unavailable)
    );
    assert_eq!(Fallback::of_mode("default"), Some(Fallback::TurnedOff));
}

#[test]
fn each_fallback_s_notice_has_its_kind_and_no_path() {
    for (fallback, kind) in [
        (Fallback::Unavailable, NoticeKind::ClaudeAutoUnavailable),
        (Fallback::TurnedOff, NoticeKind::ClaudeAutoTurnedOff),
    ] {
        let notice = fallback.notice();
        assert_eq!(notice.kind, Some(kind));
        assert_eq!(notice.level, NoticeLevel::Info);
        assert!(!notice.text.contains('/'), "{}", notice.text);
    }
}
