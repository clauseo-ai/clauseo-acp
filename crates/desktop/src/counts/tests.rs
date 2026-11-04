//! `count_ui`'s closed argument, and the focus that makes `time_open`.

use clauseo_counts::schema::{BreakKind, BreakThen, Feature, OnboardingStep, StrictErrorCode};
use clauseo_protocol::ErrorCode;
use serde_json::{Value, json};

use super::ui::{UiBreakKind, UiBreakThen, UiFeature, UiOnboardingStep};
use super::*;

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

/// Counts in memory (before the first act), in a data folder that is never written.
fn in_memory() -> Counts {
    let about = clauseo_counts::About {
        version: "3.0.4".parse().unwrap(),
        channel: clauseo_counts::schema::Channel::Stable,
        platform: clauseo_counts::schema::Platform::Linux,
        arch: clauseo_counts::schema::Arch::X86_64,
        language: clauseo_counts::schema::Language::En,
        install_type: clauseo_counts::schema::InstallType::PerUser,
        managed: false,
    };
    let dir = std::env::temp_dir().join(format!("clauseo-desktop-counts-{}", std::process::id()));
    Counts::open(&dir, about, true, false, 1)
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
