//! A jailed Codex's modes (R17 PR-D2c): two (one on Windows until PR6), both run in the preset,
//! a launch's policy from the switch it carries or the saved mode, a saved asking mode that
//! starts in `SAVED_ASK_FALLBACK`, and the refusals' code and words.

use clauseo_acp::{SessionMode, SessionModes};
use clauseo_protocol::{
    CompactString, ConfigCategory, ConfigChoice, ConfigOption, ConfigOptionKind, ErrorCode,
};

use super::{
    MODES, READ_ONLY, READ_ONLY_ENFORCED, SAVED_ASK_FALLBACK, allowed, check, check_launch,
    is_mode_option, keep_modes, keep_options, launch_policy, start_mode,
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
    assert_eq!(launch_mode(true), JAILED_MODE);
    // Tier C is unchanged.
    assert_eq!(launch_mode(false), ASK_MODE);
    assert_eq!(ASK_MODE, "workspace-write");
}

/// macOS and Linux offer both modes.
#[cfg(not(windows))]
#[test]
fn read_only_is_offered_where_the_jail_enforces_it() {
    const { assert!(READ_ONLY_ENFORCED) };
    assert_eq!(MODES, ["read-only", "agent-full-access"]);
    assert!(allowed(READ_ONLY));
    assert_eq!(code(check(READ_ONLY)), None);
}

/// Windows offers no Read-only until stage C's PR6 enforces it (TODO(PR6)): the mode list leaves
/// it out, a set is refused with `command.invalid`, and no launch picks it.
#[cfg(windows)]
#[test]
fn read_only_is_not_offered_on_windows_yet() {
    const { assert!(!READ_ONLY_ENFORCED) };
    assert_eq!(MODES, ["agent-full-access"]);
    assert!(!allowed(READ_ONLY));
    assert_eq!(code(check(READ_ONLY)), Some(ErrorCode::CommandInvalid));
    assert_eq!(launch_policy(Some(READ_ONLY), None), JAILED_MODE);
    assert_eq!(start_mode(Some(READ_ONLY)), JAILED_MODE);
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
fn only_the_offered_modes_are_allowed_and_every_other_is_refused_with_its_code() {
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

/// The refusals' words name no mode id (the orchestrator review's L4).
#[test]
fn the_refusals_name_no_mode_id() {
    for mode in ASKING.into_iter().chain([READ_ONLY]) {
        let Err(error) = check(mode) else {
            continue;
        };
        let words = error.to_string();
        for id in CODEX_MODES.into_iter().chain(ASKING) {
            assert!(!words.contains(id), "{mode}: {words}");
        }
        assert!(words.contains("stayed as it was"), "{words}");
    }
}

/// A launch's policy: the lawyer's switch it carries, when offered; else the saved mode as
/// `start_mode` reads it. An asking switch never picks a policy of its own.
#[test]
fn a_launch_takes_the_switch_it_carries_else_the_saved_mode() {
    for saved in [
        None,
        Some(READ_ONLY),
        Some(JAILED_MODE),
        Some("workspace-write"),
    ] {
        for mode in MODES {
            assert_eq!(launch_policy(Some(mode), saved), *mode);
        }
        let fallback = start_mode(saved);
        for asked in ASKING.into_iter().map(Some).chain([None]) {
            assert_eq!(launch_policy(asked, saved), fallback, "{asked:?}");
        }
    }
    assert_eq!(
        launch_policy(Some("workspace-write"), Some("agent")),
        SAVED_ASK_FALLBACK
    );
}

/// A saved asking mode starts in [`SAVED_ASK_FALLBACK`], today the jailed preset (Rohan's ruling,
/// 2026-11-09: no Codex mode switch at launch); nothing saved starts in the preset, as a new
/// thread does, whether or not the thread had a session.
#[test]
fn a_saved_asking_mode_starts_in_the_fallback_and_nothing_saved_in_the_preset() {
    assert_eq!(SAVED_ASK_FALLBACK, JAILED_MODE);
    for saved in ASKING {
        assert_eq!(start_mode(Some(saved)), SAVED_ASK_FALLBACK, "{saved}");
    }
    if READ_ONLY_ENFORCED {
        assert_eq!(start_mode(Some(READ_ONLY)), READ_ONLY);
    }
    assert_eq!(start_mode(Some(JAILED_MODE)), JAILED_MODE);
    assert_eq!(start_mode(None), JAILED_MODE);
}

/// Whatever is saved, the start is one of the offered modes.
#[test]
fn a_jailed_start_is_never_an_asking_mode() {
    let saved = CODEX_MODES
        .into_iter()
        .chain(ASKING)
        .map(Some)
        .chain([None]);
    for saved in saved {
        let mode = start_mode(saved);
        assert_eq!(code(check(mode)), None, "{saved:?} gave {mode}");
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
fn the_mode_list_and_the_mode_options_are_exactly_the_offered_ones() {
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
    keep_options(&mut options, Some(JAILED_MODE));
    for option in &options[..2] {
        assert_eq!(
            values(option),
            (
                JAILED_MODE.to_owned(),
                MODES.iter().map(|mode| (*mode).to_owned()).collect()
            ),
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
