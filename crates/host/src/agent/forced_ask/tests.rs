use clauseo_protocol::{FolderSupportKind, PermissionKind, PermissionOption};

use super::{applies, keep_option, mode_allowed};
use crate::agent::folder_support::Answer;

fn option(id: &str, kind: PermissionKind) -> PermissionOption {
    PermissionOption {
        option_id: id.into(),
        kind,
        agent_label: id.to_owned(),
    }
}

#[test]
fn a_network_drive_a_drive_without_permissions_or_a_failed_check_forces_ask() {
    assert!(applies(Answer::Known(Some(FolderSupportKind::Network))));
    assert!(applies(Answer::Known(Some(
        FolderSupportKind::NoPermissions
    ))));
    assert!(!applies(Answer::Known(Some(
        FolderSupportKind::CantChangePermissions
    ))));
    assert!(!applies(Answer::Known(Some(FolderSupportKind::Unknown))));
    assert!(!applies(Answer::Known(None)));
    // A check that failed is unknown, and unknown fails closed.
    assert!(applies(Answer::Failed));
}

#[test]
fn only_default_and_plan_may_be_set() {
    assert!(mode_allowed("default"));
    assert!(mode_allowed("plan"));
    for mode in [
        "acceptEdits",
        "bypassPermissions",
        "dontAsk",
        "auto",
        "Default",
        "x",
        "",
    ] {
        assert!(!mode_allowed(mode), "{mode}");
    }
}

/// claude-agent-acp's plan exit (`ExitPlanMode`): "Yes, and auto-accept edits" (`acceptEdits`,
/// `allow_always`) goes; "Yes, and manually approve edits" (`default`) and "No, keep planning"
/// (`plan`) stay. A bypass option, offered when bypass is allowed, goes even as `allow_once`.
#[test]
fn the_plan_exit_loses_its_auto_accept_option() {
    let options = [
        option("acceptEdits", PermissionKind::AllowAlways),
        option("bypassPermissions", PermissionKind::AllowOnce),
        option("default", PermissionKind::AllowOnce),
        option("plan", PermissionKind::RejectOnce),
    ];
    let kept: Vec<&str> = options
        .iter()
        .filter(|option| keep_option(option))
        .map(|option| option.option_id.as_str())
        .collect();
    assert_eq!(kept, ["default", "plan"]);
}

#[test]
fn a_tool_request_loses_allow_always_and_keeps_the_rest() {
    let options = [
        option("allow", PermissionKind::AllowOnce),
        option("allow_always", PermissionKind::AllowAlways),
        option("reject", PermissionKind::RejectOnce),
        option("reject_always", PermissionKind::RejectAlways),
    ];
    let kept: Vec<&str> = options
        .iter()
        .filter(|option| keep_option(option))
        .map(|option| option.option_id.as_str())
        .collect();
    assert_eq!(kept, ["allow", "reject", "reject_always"]);
}
