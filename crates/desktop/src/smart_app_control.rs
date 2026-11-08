//! Windows' Smart App Control, as the page's one-time note needs it (`display_info`'s
//! `smart_app_control`). Its state is the DWORD `VerifiedAndReputablePolicyState` under
//! `HKLM\SYSTEM\CurrentControlSet\Control\CI\Policy`: 1 On, 2 Evaluation, 0 Off (Microsoft's
//! "Application Control for Windows" and "Test app signatures with Smart App Control" pages).
//! `HKLM\SYSTEM` grants the Users group read, so the lawyer's unelevated app can read it; a
//! missing or unreadable value is unknown, and the page shows nothing.
//!
//! Clauseo is unsigned, so a running Clauseo means Off or Evaluation; only Evaluation matters,
//! because Windows later switches it to On or Off by itself, and On blocks Clauseo from then on.
//!
//! It is read once per run, off the UI thread: the `admin-refresh` thread reads it at start-up
//! (`admin_refresh.rs`), and `display_info`, on an async worker, takes that answer, or reads it
//! first when the page asks sooner. Either way one registry read and one host.log line
//! (target [`TARGET`]) with the state and nothing else. Nothing waits on it but that one read.

#[cfg(windows)]
mod windows;

use serde::Serialize;

/// The log target: host.log keeps `clauseo_*::lifecycle` INFO lines.
#[cfg(windows)]
const TARGET: &str = "clauseo_desktop::startup::lifecycle";

/// Smart App Control's state, as `display_info` reports it; `None` there when unknown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "lowercase")]
pub(crate) enum SmartAppControl {
    /// On: it blocks apps it doesn't trust, unsigned Clauseo among them.
    On,
    /// Evaluation: it blocks nothing yet, and later turns itself On or Off.
    Evaluation,
    /// Off.
    Off,
}

/// The state from `VerifiedAndReputablePolicyState`'s value: `None` when the value is missing,
/// unreadable (`value` is `None`) or not one of the three Windows documents.
pub(crate) fn state_of(value: Option<u32>) -> Option<SmartAppControl> {
    match value? {
        0 => Some(SmartAppControl::Off),
        1 => Some(SmartAppControl::On),
        2 => Some(SmartAppControl::Evaluation),
        _ => None,
    }
}

/// This run's state: read once, by whichever thread asks first, and logged then. `None` on
/// macOS and Linux.
pub(crate) fn state() -> Option<SmartAppControl> {
    #[cfg(windows)]
    {
        static STATE: std::sync::OnceLock<Option<SmartAppControl>> = std::sync::OnceLock::new();
        *STATE.get_or_init(|| {
            let state = state_of(windows::policy_state());
            let shown = state.map_or("unknown", |state| match state {
                SmartAppControl::On => "on",
                SmartAppControl::Evaluation => "evaluation",
                SmartAppControl::Off => "off",
            });
            tracing::info!(target: TARGET, state = shown, "smart app control");
            state
        })
    }
    // Nothing to read off Windows: unknown.
    #[cfg(not(windows))]
    state_of(None)
}

#[cfg(test)]
mod tests {
    use super::{SmartAppControl, state_of};

    #[test]
    fn the_three_documented_values_map_to_their_states() {
        assert_eq!(state_of(Some(1)), Some(SmartAppControl::On));
        assert_eq!(state_of(Some(2)), Some(SmartAppControl::Evaluation));
        assert_eq!(state_of(Some(0)), Some(SmartAppControl::Off));
    }

    #[test]
    fn any_other_value_or_none_is_unknown() {
        assert_eq!(state_of(Some(3)), None);
        assert_eq!(state_of(Some(u32::MAX)), None);
        assert_eq!(state_of(None), None);
    }

    #[test]
    fn it_reaches_the_page_in_lowercase() {
        let shown = serde_json::to_value(SmartAppControl::Evaluation).unwrap();
        assert_eq!(shown, serde_json::json!("evaluation"));
    }

    #[cfg(not(windows))]
    #[test]
    fn off_windows_it_is_unknown() {
        assert_eq!(super::state(), None);
    }
}
