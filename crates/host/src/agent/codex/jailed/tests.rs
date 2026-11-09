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

/// A start: the preset, the jail's policy, whether the lawyer is told.
type Start = (&'static str, Option<&'static str>, bool);

/// A Codex launch's start, as the host picks it: the preset `INITIAL_AGENT_MODE` names, the
/// jail's policy (`None` unjailed) and whether the attach tells the lawyer Read-only is not
/// available here. Unjailed, the saved mode is no input: `codex::launch_mode(false)` and
/// `forced_mode::codex_start` take none, and `forced_mode::read_only_notice` needs a jail.
fn start(jailed: bool, saved: Option<&str>) -> Start {
    if !jailed {
        return (launch_mode(false), None, false);
    }
    let policy = start_mode(saved);
    (
        launch_mode(true),
        Some(policy),
        super::read_only_unavailable(saved, policy),
    )
}

/// (jailed, unjailed) × saved (Read-only, an asking mode, none, the preset): unjailed always
/// starts in the ask mode with no notice; jailed in the preset, its policy Read-only only where
/// the jail enforces it, and the notice exactly where a saved Read-only cannot be kept.
#[test]
fn a_codex_start_by_jail_and_saved_mode() {
    let read_only = if READ_ONLY_ENFORCED {
        (JAILED_MODE, Some(READ_ONLY), false)
    } else {
        (JAILED_MODE, Some(JAILED_MODE), true)
    };
    let table: [(bool, Option<&str>, Start); 8] = [
        (true, Some(READ_ONLY), read_only),
        (
            true,
            Some(ASK_MODE),
            (JAILED_MODE, Some(SAVED_ASK_FALLBACK), false),
        ),
        (true, None, (JAILED_MODE, Some(JAILED_MODE), false)),
        (
            true,
            Some(JAILED_MODE),
            (JAILED_MODE, Some(JAILED_MODE), false),
        ),
        (false, Some(READ_ONLY), (ASK_MODE, None, false)),
        (false, Some(ASK_MODE), (ASK_MODE, None, false)),
        (false, None, (ASK_MODE, None, false)),
        (false, Some(JAILED_MODE), (ASK_MODE, None, false)),
    ];
    for (jailed, saved, expected) in table {
        assert_eq!(
            start(jailed, saved),
            expected,
            "jailed {jailed}, saved {saved:?}"
        );
    }
}

/// On every system: no unjailed Codex launch starts in the full-access preset, whatever the
/// thread saved, in its `INITIAL_AGENT_MODE` with `HOME` set or not.
#[test]
fn no_unjailed_codex_launch_starts_in_full_access() {
    assert_ne!(launch_mode(false), JAILED_MODE);
    for saved in CODEX_MODES.into_iter().map(Some).chain([None]) {
        assert_ne!(start(false, saved).0, JAILED_MODE, "{saved:?}");
    }
    let home = std::path::Path::new("state");
    for unix in [true, false] {
        let env = crate::agent::codex::launch_env_on(
            home,
            &std::collections::BTreeMap::new(),
            serde_json::Map::new(),
            None,
            unix,
        );
        let mode = env.get(std::ffi::OsStr::new(crate::agent::codex::MODE_VAR));
        assert_eq!(
            mode,
            Some(&std::ffi::OsString::from(ASK_MODE)),
            "unix {unix}"
        );
    }
}

/// The notice's rule on both kinds of system: only where Read-only is not enforced, the thread
/// was last in it, and the launch is not.
#[test]
fn read_only_unavailable_only_where_it_is_not_enforced() {
    use super::read_only_unavailable_on as told;
    for enforced in [true, false] {
        assert_eq!(told(enforced, Some(READ_ONLY), JAILED_MODE), !enforced);
        assert!(!told(enforced, Some(READ_ONLY), READ_ONLY));
        for saved in [Some(ASK_MODE), Some(JAILED_MODE), Some("agent"), None] {
            assert!(!told(enforced, saved, JAILED_MODE), "{saved:?}");
        }
    }
}
