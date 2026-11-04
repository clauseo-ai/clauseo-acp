use clauseo_protocol::{FolderSupportKind, PermissionKind, PermissionOption};

use super::{applies, config_allowed, keep_option, mode_allowed};
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
    // A check that failed is unknown, and unknown fails closed; the row's name for it too.
    assert!(applies(Answer::Failed));
    assert_eq!(
        Answer::Failed.row_kind(),
        Some(FolderSupportKind::CheckFailed)
    );
    assert!(applies(Answer::Known(Some(FolderSupportKind::CheckFailed))));
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

/// claude-agent-acp 0.89.1's plan exit (`ExitPlanMode`), every option it can offer
/// (`permissions/options/tools.js`, ids from `shared.js`): all but "Yes, manually approve edits"
/// (`exit-plan-default`) and "No, keep planning" (`reject`) switch to a skipping mode, and all
/// of those are `allow_always`. The review's
/// `rev_real_plan_exit_options_keep_only_manual_and_reject`.
#[test]
fn real_plan_exit_options_keep_only_manual_and_reject() {
    let options = [
        option("exit-plan-clear-auto", PermissionKind::AllowAlways),
        option("exit-plan-clear-bypass", PermissionKind::AllowAlways),
        option("exit-plan-clear-accept-edits", PermissionKind::AllowAlways),
        option("exit-plan-auto", PermissionKind::AllowAlways),
        option("exit-plan-bypass", PermissionKind::AllowAlways),
        option("exit-plan-accept-edits", PermissionKind::AllowAlways),
        option("exit-plan-default", PermissionKind::AllowOnce),
        option("reject", PermissionKind::RejectOnce),
    ];
    let kept: Vec<&str> = options
        .iter()
        .filter(|option| keep_option(option))
        .map(|option| option.option_id.as_str())
        .collect();
    assert_eq!(kept, ["exit-plan-default", "reject"]);
}

/// The id list is the second check: a skipping option that came as `allow_once` goes too.
#[test]
fn a_skipping_plan_exit_option_goes_whatever_its_kind() {
    for id in [
        "exit-plan-auto",
        "exit-plan-bypass",
        "exit-plan-accept-edits",
    ] {
        assert!(!keep_option(&option(id, PermissionKind::AllowOnce)), "{id}");
    }
}

/// `SetConfigOption` for the mode takes only `default` and `plan`; any other option any value.
#[test]
fn the_mode_config_option_takes_only_the_ask_modes() {
    assert!(config_allowed("mode", Some("default")));
    assert!(config_allowed("mode", Some("plan")));
    for value in [
        Some("acceptEdits"),
        Some("bypassPermissions"),
        Some("auto"),
        None,
    ] {
        assert!(!config_allowed("mode", value), "{value:?}");
    }
    assert!(config_allowed("model", Some("claude-haiku-5-5")));
    assert!(config_allowed("fast", None));
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
