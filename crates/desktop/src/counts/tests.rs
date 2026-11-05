//! `count_ui`'s closed argument, and the focus that makes `time_open`.

use clauseo_counts::schema::{BreakKind, BreakThen, Feature, OnboardingStep, StrictErrorCode};
use clauseo_protocol::ErrorCode;
use serde_json::{Value, json};

use clauseo_counts::Mode;
use clauseo_settings::SystemFiles;

use super::ui::{UiBreakKind, UiBreakThen, UiFeature, UiOnboardingStep};
use super::*;
use crate::test_support::{Scratch, block_on};

fn parse(value: Value) -> Result<UiCount, serde_json::Error> {
    serde_json::from_value(value)
}

#[test]
fn ui_counts_read_each_event_and_refuse_anything_else() {
    assert_eq!(
        parse(json!({ "feature_used": { "feature": "versions" } })).unwrap(),
        UiCount::FeatureUsed {
            feature: UiFeature::Versions
        }
    );
    assert_eq!(
        parse(json!({ "error_shown": { "code": "agent.exited" } })).unwrap(),
        UiCount::ErrorShown {
            code: StrictErrorCode::new(ErrorCode::AgentExited).unwrap()
        }
    );
    for refused in [
        // An unknown field, beside the event's own or alone.
        json!({ "feature_used": { "feature": "versions", "matter": "Mehra" } }),
        json!({ "onboarding_step": { "step": "welcome_shown", "path": "/Users/a" } }),
        // A value outside the closed set, the host's own features included.
        json!({ "feature_used": { "feature": "undo" } }),
        json!({ "feature_used": { "feature": "restore" } }),
        json!({ "feature_used": { "feature": "citation" } }),
        json!({ "feature_used": { "feature": "Mehra divorce" } }),
        json!({ "break_shown": { "kind": "offline" } }),
        // An event the webview does not count.
        json!({ "app_opened": { "launch": "cold" } }),
        json!({ "question_sent": { "agent": "claude", "delivery": "now" } }),
        // Text where a value goes, and two events at once.
        json!({ "error_shown": { "code": 7 } }),
        json!({ "feature_used": { "feature": "versions" }, "break_shown": {} }),
        json!("feature_used"),
    ] {
        assert!(parse(refused.clone()).is_err(), "{refused}");
    }
}

/// `error_shown.code` reads as `StrictErrorCode`: a code this build does not know, and
/// `unknown` itself, are refused with the argument, never turned into `Unknown`.
#[test]
fn an_unknown_error_code_is_refused_as_it_is_read() {
    for code in ["not.a.code", "unknown", "Agent.Exited", "", "agent.exited "] {
        let refused = parse(json!({ "error_shown": { "code": code } }));
        assert!(refused.is_err(), "{code:?}: {refused:?}");
    }
}

/// Each mirrored value is spelled as the counts' own: what the webview names is what is counted.
#[test]
fn every_ui_value_is_the_counts_value_of_the_same_name() {
    fn same<A: serde::Serialize, B: serde::Serialize>(ui: A, counts: B) {
        assert_eq!(
            serde_json::to_value(ui).unwrap(),
            serde_json::to_value(counts).unwrap()
        );
    }
    for kind in [
        UiBreakKind::AgentStopped,
        UiBreakKind::SigninExpired,
        UiBreakKind::Offline,
        UiBreakKind::TermsNotice,
        UiBreakKind::PlanShutOut,
    ] {
        same(kind, BreakKind::from(kind));
    }
    for then in [
        UiBreakThen::Restarted,
        UiBreakThen::SignedIn,
        UiBreakThen::Waited,
        UiBreakThen::SwitchedAgent,
        UiBreakThen::Left,
    ] {
        same(then, BreakThen::from(then));
    }
    for feature in [
        UiFeature::Versions,
        UiFeature::Compare,
        UiFeature::OpenInWord,
        UiFeature::ContinueOtherAgent,
        UiFeature::PreparedAnswerUsed,
        UiFeature::SampleMatter,
    ] {
        same(feature, Feature::from(feature));
    }
    for step in [
        UiOnboardingStep::WelcomeShown,
        UiOnboardingStep::FolderChosen,
        UiOnboardingStep::NewMatter,
        UiOnboardingStep::SampleOpened,
        UiOnboardingStep::AgentReady,
        UiOnboardingStep::SignedIn,
        UiOnboardingStep::FirstReadDone,
        UiOnboardingStep::FirstQuestion,
    ] {
        same(step, OnboardingStep::from(step));
    }
}

fn test_about() -> clauseo_counts::About {
    clauseo_counts::About {
        version: "3.0.4".parse().unwrap(),
        channel: clauseo_counts::schema::Channel::Stable,
        platform: clauseo_counts::schema::Platform::Linux,
        arch: clauseo_counts::schema::Arch::X86_64,
        language: clauseo_counts::schema::Language::En,
        install_type: clauseo_counts::schema::InstallType::PerUser,
        managed: false,
    }
}

/// Counts in memory (before the first act), in a data folder that is never written.
fn in_memory() -> Counts {
    let dir = std::env::temp_dir().join(format!("clauseo-desktop-counts-{}", std::process::id()));
    Counts::open(&dir, test_about(), true, false, 1)
}

/// A data folder holding `settings` as its `settings.json` and the first-act marker.
fn after_first_act(test: &str, settings: &[u8]) -> (Scratch, Locations) {
    let scratch = Scratch::folder(test, &[]);
    scratch.file("settings.json", settings);
    scratch.file(clauseo_counts::FIRST_ACT, b"");
    let locations = Locations::under_root(scratch.path());
    (scratch, locations)
}

/// A settings file that does not read whole leaves counts off, though the default is on: no file
/// under `counts/`, and a tick that reads the switch on still saves and sends nothing.
#[test]
fn a_damaged_settings_file_opens_the_counts_off() {
    for (name, settings) in [
        ("truncated", &br#"{"privacy":{"counts":false"#[..]),
        ("not-json", &b"privacy = off"[..]),
        ("mistyped", &br#"{"privacy":{"counts":"no"}}"#[..]),
    ] {
        let (scratch, locations) = after_first_act(&format!("damaged-{name}"), settings);
        let effective = Effective::load(&locations, &SystemFiles, &[]);
        assert!(
            effective.settings().privacy.counts,
            "{name}: the default stands"
        );
        assert!(!counts_on(&effective), "{name}");
        let counts = open_with(&locations, &effective, test_about());
        assert_eq!(counts.mode(), Mode::Off, "{name}");
        counts.bump(clauseo_counts::schema::Event::FeatureUsed(
            clauseo_counts::schema::FeatureUsed {
                feature: Feature::Versions,
            },
        ));
        let upload = HttpsUpload::to(None);
        let noon = Time::from_unix_secs(1_793_793_600);
        block_on(tick_once(
            &counts,
            &upload,
            noon,
            std::future::ready(Some(true)),
        ));
        assert_eq!(counts.mode(), Mode::Off, "{name}");
        assert!(!scratch.path().join("counts").exists(), "{name}");
    }
    // Whole files read as written.
    let (_on, locations) = after_first_act("whole-on", br#"{"privacy":{"counts":true}}"#);
    assert!(counts_on(&Effective::load(&locations, &SystemFiles, &[])));
    let (_off, locations) = after_first_act("whole-off", br#"{"privacy":{"counts":false}}"#);
    assert!(!counts_on(&Effective::load(&locations, &SystemFiles, &[])));
    let (_other, locations) = after_first_act("other-key", br#"{"ui":{"theme":7}}"#);
    assert!(counts_on(&Effective::load(&locations, &SystemFiles, &[])));
}

/// The host turns counts on (`set_enabled(true)`, then its settings write). A tick whose read
/// of the switch was answered before that write sees off: it must not turn the counts off again.
/// An off read with the switch unmoved still turns them off.
#[test]
fn a_tick_never_turns_off_counts_switched_on_after_its_read() {
    let scratch = Scratch::folder("tick-race", &[]);
    let counts = Counts::open(scratch.path(), test_about(), true, true, 1);
    let upload = HttpsUpload::to(None);
    let noon = Time::from_unix_secs(1_793_793_600);
    counts.set_enabled(false).unwrap();
    assert_eq!(counts.mode(), Mode::Off);
    // The read answers off (the settings still say false), then the host's on lands.
    let host = counts.clone();
    let stale_read = async move {
        let answered = Some(false);
        host.set_enabled(true).unwrap();
        answered
    };
    block_on(tick_once(&counts, &upload, noon, stale_read));
    assert_eq!(
        counts.mode(),
        Mode::Saving,
        "the stale off turned counts off"
    );
    // A fresh off, with no switch since the read, turns them off.
    block_on(tick_once(
        &counts,
        &upload,
        noon.plus(60),
        std::future::ready(Some(false)),
    ));
    assert_eq!(counts.mode(), Mode::Off);
}

#[test]
fn a_ui_count_goes_into_the_counts_once_they_are_open() {
    let state = CountsState::default();
    // Before the counts open: dropped.
    state.count(UiCount::FeatureUsed {
        feature: UiFeature::Versions,
    });
    let counts = in_memory();
    state.start(counts.clone());
    state.count(UiCount::FeatureUsed {
        feature: UiFeature::Versions,
    });
    state.count(UiCount::ErrorShown {
        code: StrictErrorCode::new(ErrorCode::AgentExited).unwrap(),
    });
    let today = counts.today();
    assert_eq!(today.feature_used.len(), 1);
    assert_eq!(today.feature_used[0].n.get(), 1);
    assert_eq!(today.error_shown.len(), 1);
}

/// The setup pump's counts: dropped while the counts are absent, counted once they are open.
#[test]
fn a_setup_count_goes_into_the_counts_only_once_they_are_open() {
    use clauseo_counts::schema::{Agent, Billing, SignIn, SignInResult};
    let event = Event::SignIn(SignIn {
        agent: Agent::Claude,
        billing: Billing::Plan,
        result: SignInResult::Done,
    });
    let state = CountsState::default();
    state.bump(event);
    let counts = in_memory();
    assert_eq!(counts.today().sign_in.len(), 0);
    state.start(counts.clone());
    state.bump(event);
    let today = counts.today();
    assert_eq!(today.sign_in.len(), 1);
    assert_eq!(today.sign_in[0].n.get(), 1);
}

#[test]
fn either_window_holding_the_focus_is_the_foreground() {
    let state = CountsState::default();
    // Focus before the counts open is kept for them.
    state.focused(crate::MAIN_WINDOW, true);
    let counts = in_memory();
    state.start(counts);
    let focus = |state: &CountsState| state.focus.lock().unwrap().contains(&true);
    assert!(focus(&state));
    // From main to settings, in either order of the two events.
    state.focused(crate::settings_window::SETTINGS_WINDOW, true);
    state.focused(crate::MAIN_WINDOW, false);
    assert!(focus(&state));
    state.focused(crate::settings_window::SETTINGS_WINDOW, false);
    assert!(!focus(&state));
    // Another window's focus is not ours.
    state.focused("e2e", true);
    assert!(!focus(&state));
}

/// The files under `dir`, with their contents, sorted by path.
fn files_under(dir: &std::path::Path) -> Vec<(std::path::PathBuf, Vec<u8>)> {
    fn walk(dir: &std::path::Path, out: &mut Vec<(std::path::PathBuf, Vec<u8>)>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, out);
            } else {
                out.push((path.clone(), std::fs::read(&path).unwrap()));
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, &mut out);
    out.sort();
    out
}

/// A tick after the host has shut down (`Closed`) gets no value for the switch and changes
/// nothing: the counts keep their mode, their install id and every file under `counts/`.
#[test]
fn a_tick_after_the_host_shut_down_changes_nothing() {
    let host_scratch = Scratch::new("tick-closed-host", &[]);
    let host = host_scratch.start(Vec::new());
    let bridge = host.bridge();
    let data = Scratch::folder("tick-closed-counts", &[]);
    let counts = Counts::open(data.path(), test_about(), true, true, 1);
    let upload = HttpsUpload::to(None);
    let noon = Time::from_unix_secs(1_793_793_600);
    counts.bump(clauseo_counts::schema::Event::FeatureUsed(
        clauseo_counts::schema::FeatureUsed {
            feature: Feature::Versions,
        },
    ));
    // A live host answers: counts on (the default).
    assert_eq!(block_on(host_counts_on(&bridge.cell())), Some(true));
    block_on(tick_once(
        &counts,
        &upload,
        noon,
        host_counts_on(&bridge.cell()),
    ));
    let install = counts.install_id();
    let before = files_under(&data.path().join("counts"));
    assert!(install.is_some());
    assert!(before.len() >= 2, "install_id and today.json: {before:?}");

    block_on(host.host.shutdown()).unwrap();
    assert_eq!(block_on(host_counts_on(&bridge.cell())), None);
    block_on(tick_once(
        &counts,
        &upload,
        noon,
        host_counts_on(&bridge.cell()),
    ));
    assert_eq!(counts.mode(), Mode::Saving);
    assert_eq!(counts.install_id(), install);
    assert_eq!(files_under(&data.path().join("counts")), before);
}

/// The exit's `stop_ticker` aborts the task, which never ends by itself; a task asked to start
/// after the stop never starts.
#[test]
fn the_exit_stops_the_tick_task_for_good() {
    let host_scratch = Scratch::new("ticker-stop", &[]);
    let host = host_scratch.start(Vec::new());
    let bridge = host.bridge();
    let data = Scratch::folder("ticker-stop-counts", &[]);
    let counts = Counts::open(data.path(), test_about(), true, true, 1);
    let state = CountsState::default();
    state.start(counts.clone());
    let cell = bridge.cell();
    state.run_ticker(|| spawn_ticker(counts.clone(), HttpsUpload::to(None), cell));
    let task = state.stop_ticker().expect("the task had started");
    let ended = block_on(task);
    assert!(ended.is_err(), "the task ended by itself: {ended:?}");
    let mut started = false;
    state.run_ticker(|| {
        started = true;
        tauri::async_runtime::spawn(async {})
    });
    assert!(!started, "a task started after the stop");
    assert!(state.stop_ticker().is_none());
}
