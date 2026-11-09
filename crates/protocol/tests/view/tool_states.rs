//! A header's tool states: every reason round trips, the notice is written only when set, and
//! values a later app adds decode as `Unknown`.

use clauseo_protocol::{
    ClauseoTool, ToolAvailability, ToolNotice, ToolState, ToolsUnavailableReason,
};

use crate::entries::header_entry;
use crate::fixtures::round_trips;

#[test]
fn tool_states_round_trip_in_a_header_with_every_reason() {
    let mut entry = header_entry();
    entry.tools.push(ToolState {
        tool: ClauseoTool::WebSearch,
        availability: ToolAvailability::Available,
        notice: None,
    });
    for reason in ToolsUnavailableReason::ALL
        .into_iter()
        .filter(|reason| *reason != ToolsUnavailableReason::Unknown)
    {
        entry.tools.push(ToolState {
            tool: ClauseoTool::Browser,
            availability: ToolAvailability::Unavailable { reason },
            notice: None,
        });
    }
    round_trips(&entry);
    let json = serde_json::to_value(&entry).unwrap();
    assert_eq!(
        json["tools"][0],
        serde_json::json!({"tool": "web_search", "availability": "available"})
    );
    let reasons: Vec<&str> = json["tools"].as_array().unwrap()[1..]
        .iter()
        .map(|state| {
            assert_eq!(state["tool"], "browser");
            state["availability"]["unavailable"]["reason"]
                .as_str()
                .unwrap()
        })
        .collect();
    assert_eq!(
        reasons,
        [
            "off",
            "locked_off",
            "no_search_on_this_login",
            "managed_by_firm",
            "no_proxy",
            "browser_not_installed",
            "too_many_browsers",
            "no_relay",
            "browser_blocked_by_policy",
            "browser_policy_unreadable",
            "browser_out_of_date",
            "browser_list_unavailable",
            "browser_list_expired",
        ]
    );
}

#[test]
fn a_tool_notice_is_written_when_set_and_left_out_when_none() {
    let old = ToolState {
        tool: ClauseoTool::Browser,
        availability: ToolAvailability::Available,
        notice: Some(ToolNotice::BrowserListOld),
    };
    assert_eq!(
        serde_json::to_value(&old).unwrap(),
        serde_json::json!({
            "tool": "browser",
            "availability": "available",
            "notice": "browser_list_old",
        })
    );
    let state: ToolState = serde_json::from_str(
        r#"{"tool": "browser", "availability": "available", "notice": "comet_seen"}"#,
    )
    .unwrap();
    assert_eq!(state.notice, Some(ToolNotice::Unknown));
    let state: ToolState =
        serde_json::from_str(r#"{"tool": "browser", "availability": "available"}"#).unwrap();
    assert_eq!(state.notice, None);
    assert!(
        serde_json::to_value(&state)
            .unwrap()
            .get("notice")
            .is_none()
    );
}

#[test]
fn an_unknown_tool_reason_or_availability_decodes() {
    let state: ToolState = serde_json::from_str(
        r#"{"tool": "scanner", "availability": {"unavailable": {"reason": "solar_flare"}}}"#,
    )
    .unwrap();
    assert_eq!(state.tool, ClauseoTool::Unknown);
    assert_eq!(
        state.availability,
        ToolAvailability::Unavailable {
            reason: ToolsUnavailableReason::Unknown
        }
    );
    let state: ToolState =
        serde_json::from_str(r#"{"tool": "browser", "availability": {"paused": {"until": 3}}}"#)
            .unwrap();
    assert_eq!(state.tool, ClauseoTool::Browser);
    assert!(matches!(
        state.availability,
        ToolAvailability::Unknown { .. }
    ));
}
