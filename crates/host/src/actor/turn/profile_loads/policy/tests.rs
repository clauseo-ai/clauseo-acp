use clauseo_protocol::{ErrorCode, PolicySetting};

use super::policy_keys::weakens::Weakening;
use super::{Finding, guard_verdict, refusal};

#[test]
fn a_launch_resting_on_the_guard_is_refused_without_it_and_others_go_on() {
    // On Windows (no sandbox) or in forced ask the launch rests on the guard hook.
    for rests in [true, false] {
        let refused = |code| rests.then_some(code);
        assert_eq!(guard_verdict(rests, None, None), None);
        assert_eq!(
            guard_verdict(rests, None, Some("no_program")),
            refused(ErrorCode::AgentGuardMissing)
        );
        assert_eq!(
            guard_verdict(rests, Some("disableAllHooks"), None),
            refused(ErrorCode::AgentFirmHooksOff)
        );
        // The firm's setting is named first: a reinstall would not help.
        assert_eq!(
            guard_verdict(rests, Some("allowManagedHooksOnly"), Some("no_program")),
            refused(ErrorCode::AgentFirmHooksOff)
        );
    }
}

/// A refusal carries the setting the view names when its key is on the fixed list; a key off
/// it, or a policy document that cannot be read, names none, so neither reaches the view.
#[test]
fn a_policy_refusal_names_its_setting_only_from_the_fixed_list() {
    let weakens = |key: &'static [&'static str]| {
        Finding::Weakens(Weakening {
            key,
            source: "managed-settings.json",
        })
    };
    let firm = ErrorCode::AgentFirmPolicyWeakens;
    for (found, setting) in [
        (
            weakens(&["sandbox", "enabled"]),
            Some(PolicySetting::SandboxEnabled),
        ),
        (
            weakens(&["disableAllHooks"]),
            Some(PolicySetting::DisableAllHooks),
        ),
        (weakens(&["sandbox", "network", "httpProxyPort"]), None),
        (weakens(&["somethingElse"]), None),
        (Finding::Unreadable("managed-settings.d"), None),
    ] {
        let error = refusal(firm, found);
        assert_eq!(error.code(), firm, "{found:?}");
        assert_eq!(error.setting(), setting, "{found:?}");
    }
    let user = refusal(
        ErrorCode::AgentUserPolicy,
        weakens(&["permissions", "defaultMode"]),
    );
    assert_eq!(user.setting(), Some(PolicySetting::PermissionsDefaultMode));
}
