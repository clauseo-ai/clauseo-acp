//! `TurnFailed.setting` and `PolicySetting`: each setting is written as its dotted key, in JSON
//! and `MessagePack`; an entry written before the field existed decodes with `None`, and a key a
//! newer Clauseo adds reads as `Unknown`, which the app shows as no setting named.

use clauseo_protocol::{DomainEvent, ErrorCode, PolicySetting, RawPayload, TurnFailed};

use crate::fixtures::{entry, packed_raw, turn_id};

/// Every setting but `Unknown` with its wire string, in declaration order.
const WIRE: [(PolicySetting, &str); 22] = [
    (PolicySetting::DisableAllHooks, "disableAllHooks"),
    (
        PolicySetting::AllowManagedHooksOnly,
        "allowManagedHooksOnly",
    ),
    (
        PolicySetting::AllowManagedPermissionRulesOnly,
        "allowManagedPermissionRulesOnly",
    ),
    (
        PolicySetting::SkipDangerousModePermissionPrompt,
        "skipDangerousModePermissionPrompt",
    ),
    (PolicySetting::DisableBundledSkills, "disableBundledSkills"),
    (
        PolicySetting::PermissionsDefaultMode,
        "permissions.defaultMode",
    ),
    (
        PolicySetting::PermissionsDisableBypassPermissionsMode,
        "permissions.disableBypassPermissionsMode",
    ),
    (PolicySetting::SandboxEnabled, "sandbox.enabled"),
    (
        PolicySetting::SandboxAutoAllowBashIfSandboxed,
        "sandbox.autoAllowBashIfSandboxed",
    ),
    (
        PolicySetting::SandboxAllowUnsandboxedCommands,
        "sandbox.allowUnsandboxedCommands",
    ),
    (
        PolicySetting::SandboxFailIfUnavailable,
        "sandbox.failIfUnavailable",
    ),
    (
        PolicySetting::SandboxAllowAppleEvents,
        "sandbox.allowAppleEvents",
    ),
    (
        PolicySetting::SandboxEnableWeakerNestedSandbox,
        "sandbox.enableWeakerNestedSandbox",
    ),
    (
        PolicySetting::SandboxEnableWeakerNetworkIsolation,
        "sandbox.enableWeakerNetworkIsolation",
    ),
    (
        PolicySetting::SandboxNetworkAllowLocalBinding,
        "sandbox.network.allowLocalBinding",
    ),
    (
        PolicySetting::SandboxNetworkAllowAllUnixSockets,
        "sandbox.network.allowAllUnixSockets",
    ),
    (
        PolicySetting::SandboxExcludedCommands,
        "sandbox.excludedCommands",
    ),
    (
        PolicySetting::IncludeGitInstructions,
        "includeGitInstructions",
    ),
    (PolicySetting::CrossSessionInbound, "crossSessionInbound"),
    (PolicySetting::DisableRemoteControl, "disableRemoteControl"),
    (PolicySetting::Tools, "tools"),
    (PolicySetting::EnvPath, "env.PATH"),
];

fn failed(setting: Option<PolicySetting>) -> TurnFailed {
    TurnFailed {
        turn: turn_id(3),
        code: ErrorCode::AgentFirmPolicyWeakens,
        detail: None,
        setting,
    }
}

#[test]
fn every_setting_is_written_as_its_dotted_key() {
    let known: Vec<_> = PolicySetting::ALL
        .into_iter()
        .filter(|setting| *setting != PolicySetting::Unknown)
        .collect();
    assert_eq!(known, WIRE.map(|(setting, _)| setting));
    for (setting, wire) in WIRE {
        assert_eq!(serde_json::to_value(setting).unwrap(), wire);
        let event = DomainEvent::from(failed(Some(setting)));
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["turn_failed"]["setting"], wire);
        let back: DomainEvent = serde_json::from_value(json).unwrap();
        assert_eq!(back, event, "{wire}");
        let packed: DomainEvent =
            rmp_serde::from_slice(&rmp_serde::to_vec_named(&event).unwrap()).unwrap();
        assert_eq!(packed, event, "{wire}");
    }
}

#[test]
fn no_setting_is_left_out_and_an_older_entry_reads_none() {
    let json = serde_json::to_value(failed(None)).unwrap();
    assert!(json.get("setting").is_none(), "{json}");
    let read: TurnFailed = serde_json::from_value(json).unwrap();
    assert_eq!(read, failed(None));
}

#[test]
fn a_key_a_newer_clauseo_adds_reads_as_unknown() {
    for key in ["sandbox.network.httpProxyPort", "somethingElse", "unknown"] {
        let mut json = serde_json::to_value(failed(Some(PolicySetting::SandboxEnabled))).unwrap();
        json["setting"] = key.into();
        let read: TurnFailed = serde_json::from_value(json).unwrap();
        assert_eq!(read.setting, Some(PolicySetting::Unknown), "JSON {key}");
        let mut packed = packed_raw(&failed(Some(PolicySetting::SandboxEnabled)));
        *entry(&mut packed, "setting") = RawPayload::Str(key.into());
        let bytes = rmp_serde::to_vec_named(&packed).unwrap();
        let read: TurnFailed = rmp_serde::from_slice(&bytes).unwrap();
        assert_eq!(
            read.setting,
            Some(PolicySetting::Unknown),
            "MessagePack {key}"
        );
    }
}
