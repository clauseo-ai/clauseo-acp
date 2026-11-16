//! The `SessionStart` settings canary's entry in a Claude session's flag settings
//! (`claudeCode.options.settings.hooks.SessionStart`), beside the guard's `PreToolUse` entry
//! ([`super::guard_entry`]): the same program (the desktop executable), in the same exec form
//! (`command` plus `args`, no shell), with `--pre-write-guard --session-canary <file> <nonce>`
//! (`crate::pre_write_guard`'s canary mode, which writes the nonce to the file and exits).
//!
//! Claude Code 2.1.293 runs its `SessionStart` hooks before claude-agent-acp 0.89.1 answers
//! `session/new`, `session/load` or `session/resume` (and `new` with `options.resume`), and the
//! answer waits for them. So a nonce in the file after the answer proves the settings loaded;
//! `actor/turn/session/canary.rs` arms it per session call and reads it back.
//!
//! Pure: [`canary_entry`] builds the entry and [`add_canary`] puts it in a `_meta` that
//! [`super::session_meta`] built; the caller picks the nonce and the file.

use std::path::Path;

use serde_json::{Map, Value};

use super::take_object;

/// The `SessionStart` sources the canary runs for: a new session (`startup`) and one taken back
/// (`resume`: `session/load`, `session/resume`, and `new` with `options.resume`). Not `clear`
/// or `compact`, which fire mid-session, after the check, where a second run would find its
/// file already written and fail.
pub(crate) const CANARY_MATCHER: &str = "startup|resume";

/// The seconds Claude Code waits for the canary (its default for a command hook is 600). The
/// session call's answer waits for it, so it is short: a warm run added 33 ms to `session/new`
/// (Claude Code 2.1.293, Linux). A canary cut off by it,
/// or one that exits non-zero, leaves no nonce (a timed-out hook never writes late), and the
/// host refuses the session after its one retry, as for one that never ran.
///
/// Measured on the laptop, 16 Nov (Win 11 26H2, Defender real-time on): cold right after a
/// reboot 3,106 ms; cold later 484 ms; warm ~16 ms. 10 s is about 3x the worst seen, because a
/// lawyer's PC can be slower (an older disk, third-party antivirus). A long limit costs nothing
/// normally (the wait is the hook's real run) and only delays refusing a truly hung canary.
pub(crate) const CANARY_TIMEOUT_SECONDS: u64 = 10;

/// What one session call's canary needs: the program, the file it writes and the nonce.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CanaryHook<'a> {
    /// The guard's program (`HostConfig::with_pre_write_guard`).
    pub(crate) program: &'a Path,
    /// The nonce file, new for each session call, in the session's run folder.
    pub(crate) file: &'a Path,
    /// The nonce.
    pub(crate) nonce: &'a str,
}

/// The `SessionStart` entry that runs `program` with
/// `--pre-write-guard --session-canary <file> <nonce>` for [`CANARY_MATCHER`]'s sources, in the
/// exec form [`super::guard_entry`] uses. `None` when `program` or `file` is no UTF-8 string:
/// JSON cannot carry it.
pub(crate) fn canary_entry(hook: CanaryHook<'_>) -> Option<Value> {
    let (Some(command), Some(file)) = (hook.program.to_str(), hook.file.to_str()) else {
        return None;
    };
    Some(serde_json::json!({
        "matcher": CANARY_MATCHER,
        "hooks": [{
            "type": "command",
            "command": command,
            "args": [
                crate::pre_write_guard::FLAG,
                crate::pre_write_guard::CANARY_FLAG,
                file,
                hook.nonce,
            ],
            "timeout": CANARY_TIMEOUT_SECONDS,
        }],
    }))
}

/// `entry` appended to `meta`'s `claudeCode.options.settings.hooks.SessionStart`, after any
/// entries already there; a value at `claudeCode`, `options`, `settings` or `hooks` that is not
/// an object, or at `SessionStart` that is not an array, is replaced.
pub(crate) fn add_canary(meta: &mut Map<String, Value>, entry: Value) {
    let mut claude = take_object(meta, "claudeCode");
    let mut options = take_object(&mut claude, "options");
    let mut settings = take_object(&mut options, "settings");
    let mut hooks = take_object(&mut settings, "hooks");
    let mut entries = match hooks.remove("SessionStart") {
        Some(Value::Array(entries)) => entries,
        _ => Vec::new(),
    };
    entries.push(entry);
    hooks.insert("SessionStart".to_owned(), Value::Array(entries));
    settings.insert("hooks".to_owned(), Value::Object(hooks));
    options.insert("settings".to_owned(), Value::Object(settings));
    claude.insert("options".to_owned(), Value::Object(options));
    meta.insert("claudeCode".to_owned(), Value::Object(claude));
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use serde_json::{Map, Value, json};

    use super::{CanaryHook, add_canary, canary_entry};

    // unix-paths-ok: planted values, never opened.
    const PROGRAM: &str = "/opt/clauseo/clauseo-desktop";
    // unix-paths-ok: planted values, never opened.
    const FILE: &str = "/data/run/s1/session-canary-1";

    fn hook<'a>(program: &'a Path, file: &'a Path) -> CanaryHook<'a> {
        CanaryHook {
            program,
            file,
            nonce: "0123abcd",
        }
    }

    #[test]
    fn the_entry_runs_the_guard_program_in_its_canary_mode() {
        let entry = canary_entry(hook(Path::new(PROGRAM), Path::new(FILE))).unwrap();
        assert_eq!(
            entry,
            json!({
                "matcher": "startup|resume",
                "hooks": [{
                    "type": "command",
                    "command": PROGRAM,
                    "args": ["--pre-write-guard", "--session-canary", FILE, "0123abcd"],
                    "timeout": 10,
                }],
            })
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_path_that_is_no_utf8_gives_no_entry() {
        use std::os::unix::ffi::OsStrExt as _;
        let bad = Path::new(std::ffi::OsStr::from_bytes(b"/opt/\xff"));
        assert_eq!(canary_entry(hook(bad, Path::new(FILE))), None);
        assert_eq!(canary_entry(hook(Path::new(PROGRAM), bad)), None);
    }

    #[test]
    fn the_entry_goes_after_the_hooks_own_session_start_entries() {
        let own = json!({"hooks": [{"type": "command", "command": "/opt/firm/start"}]});
        let mut meta: Map<String, Value> = json!({"claudeCode": {"options": {"settings": {
            "hooks": {"SessionStart": [own], "PreToolUse": [{"matcher": "Edit"}]},
            "permissions": {"deny": ["Read(//x)"]},
        }}}})
        .as_object()
        .cloned()
        .unwrap();
        add_canary(&mut meta, json!({"canary": 1}));
        let settings = &meta["claudeCode"]["options"]["settings"];
        assert_eq!(
            settings["hooks"]["SessionStart"],
            json!([own, {"canary": 1}])
        );
        assert_eq!(
            settings["hooks"]["PreToolUse"],
            json!([{"matcher": "Edit"}])
        );
        assert_eq!(settings["permissions"], json!({"deny": ["Read(//x)"]}));
    }

    #[test]
    fn a_value_of_another_type_is_replaced() {
        for meta in [
            json!({}),
            json!({"claudeCode": "x"}),
            json!({"claudeCode": {"options": {"settings": {"hooks": "none"}}}}),
            json!({"claudeCode": {"options": {"settings": {"hooks": {"SessionStart": {}}}}}}),
        ] {
            let mut meta = meta.as_object().cloned().unwrap();
            add_canary(&mut meta, json!({"canary": 1}));
            assert_eq!(
                meta["claudeCode"]["options"]["settings"]["hooks"]["SessionStart"],
                json!([{"canary": 1}])
            );
        }
    }
}
