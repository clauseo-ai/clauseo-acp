//! The guard's config-change mode ([`run`]), Claude Code's `ConfigChange` hook in every Clauseo
//! session's flag settings (`agent::loads::config_change_entry`): a running session reloads no
//! user, project or local settings file and no skills folder that changed after its start. The
//! hook input's `source` names what changed; each of [`BLOCKED`] is refused (exit
//! [`super::DENY`]), and so is an input that cannot be read. Any other source goes through:
//! `policy_settings`, a firm's managed settings, which Claude Code never lets a hook block.
//!
//! Claude Code applies a reload whose hook cannot start, exits with another code or times out;
//! the launch self-test runs the entry on [`selftest_input`] and refuses the launch unless it
//! refuses that.

use std::io::{Read, Write};

/// The argument after the guard's flag that selects this mode.
pub const FLAG: &str = "--config-change";

/// The `source` values of Claude Code 2.1.293's `ConfigChange` event refused here.
pub const BLOCKED: [&str; 4] = [
    "user_settings",
    "project_settings",
    "local_settings",
    "skills",
];

/// The hook entry's matcher: [`BLOCKED`] joined by `|`, an exact list.
pub const MATCHER: &str = "user_settings|project_settings|local_settings|skills";

/// The reason on stderr with a refusal.
pub const REASON: &str =
    "Clauseo keeps a running session's settings and skills as they were when it started";

/// The most input read: a hook object is a few hundred bytes.
const MAX_INPUT: u64 = 64 * 1024;

/// Reads one `ConfigChange` hook input from `input`; [`super::DENY`] with [`REASON`] on
/// `errors` when its `source` is one of [`BLOCKED`] or it cannot be read, else 0 with nothing
/// written.
pub(super) fn run(input: impl Read, mut errors: impl Write) -> i32 {
    let mut bytes = Vec::new();
    let source = input
        .take(MAX_INPUT)
        .read_to_end(&mut bytes)
        .ok()
        .and_then(|_| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .and_then(|hook| hook.get("source")?.as_str().map(str::to_owned));
    if source.is_some_and(|source| !BLOCKED.contains(&source.as_str())) {
        return 0;
    }
    // When the agent stopped reading, the exit code still decides.
    drop(
        errors
            .write_all(REASON.as_bytes())
            .and_then(|()| errors.write_all(b"\n"))
            .and_then(|()| errors.flush()),
    );
    super::DENY
}

/// A made-up input this mode must refuse: a change to the user settings file, as Claude Code
/// sends it. For the launch self-test of the hook entry.
pub fn selftest_input() -> Vec<u8> {
    serde_json::json!({
        "session_id": "clauseo-selftest",
        "hook_event_name": "ConfigChange",
        "source": "user_settings",
        "file_path": "settings.json",
    })
    .to_string()
    .into_bytes()
}

#[cfg(test)]
mod tests {
    use super::{BLOCKED, MATCHER, REASON, run, selftest_input};

    /// The exit code and stderr of a run on `input`.
    fn ask(input: &[u8]) -> (i32, String) {
        let mut errors = Vec::new();
        let code = run(input, &mut errors);
        (code, String::from_utf8(errors).unwrap())
    }

    /// Each blocked source and the self-test input are refused with the reason; a firm's
    /// managed settings and any other source go through; an input that cannot be read, or has
    /// no source, is refused. The matcher names the blocked sources and nothing else.
    #[test]
    fn a_change_to_settings_or_skills_is_refused_and_managed_settings_go_through() {
        for source in BLOCKED {
            let input = serde_json::json!({"hook_event_name": "ConfigChange", "source": source});
            assert_eq!(
                ask(input.to_string().as_bytes()),
                (2, format!("{REASON}\n"))
            );
        }
        assert_eq!(ask(&selftest_input()).0, 2);
        for source in ["policy_settings", "something_new"] {
            let input = serde_json::json!({"hook_event_name": "ConfigChange", "source": source});
            assert_eq!(
                ask(input.to_string().as_bytes()),
                (0, String::new()),
                "{source}"
            );
        }
        for input in [&b""[..], b"not json", b"{}", br#"{"source": 1}"#] {
            assert_eq!(ask(input).0, 2);
        }
        assert_eq!(MATCHER.split('|').collect::<Vec<_>>(), BLOCKED);
        assert!(!MATCHER.contains("policy"));
    }
}
