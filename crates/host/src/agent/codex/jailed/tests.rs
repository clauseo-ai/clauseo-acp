//! A jailed Codex's modes (R17 PR-D2c): exactly two, both run in the preset, a launch's policy
//! from the switch it carries or the saved mode, a saved asking mode that starts read-only, and
//! the refusals' code.

use clauseo_acp::{SessionMode, SessionModes};
use clauseo_protocol::{
    CompactString, ConfigCategory, ConfigChoice, ConfigOption, ConfigOptionKind, ErrorCode,
};

use super::{
    MODES, READ_ONLY, allowed, check, check_launch, is_mode_option, keep_modes, keep_options,
    launch_policy, start_mode,
};
use crate::agent::codex::{ASK_MODE, JAILED_MODE, launch_mode};

/// codex-acp 2.2.2's four presets (`src/AgentMode.ts`, `dist/index.js:34243-34354`).
const CODEX_MODES: [&str; 4] = ["read-only", "workspace-write", "agent", "agent-full-access"];

/// Every mode that is not one of a jailed Codex's two, an unknown one included.
const ASKING: [&str; 4] = ["workspace-write", "agent", "default", "something-new"];

fn code(result: Result<(), crate::error::Error>) -> Option<ErrorCode> {
    result.err().map(|error| error.code())
}

#[test]
fn the_jailed_preset_is_pinned_and_a_jailed_launch_starts_in_it() {
    assert_eq!(JAILED_MODE, "agent-full-access");
    assert_eq!(READ_ONLY, "read-only");
    assert_eq!(MODES, ["read-only", "agent-full-access"]);
    assert_eq!(launch_mode(true), JAILED_MODE);
    // Tier C is unchanged.
    assert_eq!(launch_mode(false), ASK_MODE);
    assert_eq!(ASK_MODE, "workspace-write");
}

/// No launcher for a jailed Codex in any mode but the preset (`policy::grants`).
#[test]
fn a_jailed_launch_in_any_other_mode_is_refused() {
    assert_eq!(code(check_launch(launch_mode(true))), None);
    for mode in CODEX_MODES.into_iter().filter(|mode| *mode != JAILED_MODE) {
        assert_eq!(
            code(check_launch(mode)),
            Some(ErrorCode::SandboxAskModeUnavailable),
            "{mode}"
        );
    }
}

#[test]
fn only_the_two_modes_are_allowed_and_every_other_is_refused_with_its_code() {
    for mode in MODES {
        assert!(allowed(mode));
        assert_eq!(code(check(mode)), None, "{mode}");
    }
    for mode in ASKING.into_iter().chain([""]) {
        assert!(!allowed(mode), "{mode}");
        assert_eq!(
            code(check(mode)),
            Some(ErrorCode::SandboxAskModeUnavailable),
            "{mode}"
        );
    }
    // By exact id.
    assert!(!allowed("Read-only"));
    assert!(!allowed("agent-full-access "));
}

/// A launch's policy: the lawyer's switch it carries, when one of the two; else the saved mode
/// as `start_mode` reads it. An asking switch never picks the normal policy.
#[test]
fn a_launch_takes_the_switch_it_carries_else_the_saved_mode() {
    for saved in [
        None,
        Some(READ_ONLY),
        Some(JAILED_MODE),
        Some("workspace-write"),
    ] {
        for had_session in [true, false] {
            for mode in MODES {
                assert_eq!(launch_policy(Some(mode), saved, had_session), mode);
            }
            let fallback = start_mode(saved, had_session);
            for asked in ASKING.into_iter().map(Some).chain([None]) {
                assert_eq!(
                    launch_policy(asked, saved, had_session),
                    fallback,
                    "{asked:?}"
                );
            }
        }
    }
    assert_eq!(
        launch_policy(Some("workspace-write"), Some("agent"), true),
        READ_ONLY
    );
}

#[test]
fn a_saved_asking_mode_starts_read_only_never_in_the_preset() {
    for saved in ASKING {
        for had_session in [true, false] {
            assert_eq!(start_mode(Some(saved), had_session), READ_ONLY, "{saved}");
        }
    }
    assert_eq!(start_mode(Some(READ_ONLY), true), READ_ONLY);
    assert_eq!(start_mode(Some(JAILED_MODE), true), JAILED_MODE);
    // Nothing saved: a new thread starts in the preset; one whose mode can't be read, read-only.
    assert_eq!(start_mode(None, false), JAILED_MODE);
    assert_eq!(start_mode(None, true), READ_ONLY);
}

/// Whatever is saved, the start is one of the two.
#[test]
fn a_jailed_start_is_never_an_asking_mode() {
    let saved = CODEX_MODES
        .into_iter()
        .chain(ASKING)
        .map(Some)
        .chain([None]);
    for saved in saved {
        for had_session in [true, false] {
            let mode = start_mode(saved, had_session);
            assert_eq!(code(check(mode)), None, "{saved:?} gave {mode}");
        }
    }
}

fn choice(value: &str) -> ConfigChoice {
    ConfigChoice {
        value: CompactString::from(value),
        name: CompactString::from(value),
        description: None,
        group: None,
    }
}

fn select(id: &str, category: ConfigCategory, current: &str, values: &[&str]) -> ConfigOption {
    ConfigOption {
        id: CompactString::from(id),
        name: CompactString::from(id),
        description: None,
        category,
        kind: ConfigOptionKind::Select {
            current_value: CompactString::from(current),
            options: values.iter().map(|value| choice(value)).collect(),
        },
    }
}

fn values(option: &ConfigOption) -> (String, Vec<String>) {
    match &option.kind {
        ConfigOptionKind::Select {
            current_value,
            options,
        } => (
            current_value.to_string(),
            options
                .iter()
                .map(|choice| choice.value.to_string())
                .collect(),
        ),
        other @ (ConfigOptionKind::Boolean { .. } | ConfigOptionKind::Unknown { .. }) => {
            panic!("not a select: {other:?}")
        }
    }
}

#[test]
fn the_mode_list_and_the_mode_options_are_exactly_the_two() {
    let mut modes = SessionModes {
        current: CompactString::from("agent"),
        available: CODEX_MODES
            .iter()
            .map(|id| SessionMode {
                id: CompactString::from(*id),
                name: (*id).to_owned(),
                description: None,
            })
            .collect(),
    };
    keep_modes(&mut modes);
    let ids: Vec<&str> = modes
        .available
        .iter()
        .map(|mode| mode.id.as_str())
        .collect();
    assert_eq!(ids, MODES);

    let mut options = vec![
        select("mode", ConfigCategory::Other, "agent", &CODEX_MODES),
        select("approvals", ConfigCategory::Mode, "agent", &CODEX_MODES),
        select("model", ConfigCategory::Model, "gpt", &["gpt", "agent"]),
    ];
    keep_options(&mut options, Some(READ_ONLY));
    for option in &options[..2] {
        assert_eq!(
            values(option),
            (READ_ONLY.to_owned(), MODES.map(str::to_owned).to_vec()),
            "{}",
            option.id
        );
    }
    // Any other option is left as it is.
    assert_eq!(
        values(&options[2]),
        ("gpt".to_owned(), vec!["gpt".to_owned(), "agent".to_owned()])
    );
    // Without a current value, the current one stays.
    let mut later = vec![select(
        "mode",
        ConfigCategory::Mode,
        JAILED_MODE,
        &CODEX_MODES,
    )];
    keep_options(&mut later, None);
    assert_eq!(values(&later[0]).0, JAILED_MODE);
    assert_eq!(values(&later[0]).1, MODES);

    assert!(is_mode_option("mode", &[]));
    assert!(is_mode_option("approvals", &options));
    assert!(!is_mode_option("model", &options));
}
