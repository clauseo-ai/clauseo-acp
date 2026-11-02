//! The pre-write guard's hook for Codex, for R15 to merge into `CODEX_CONFIG` (codex-acp passes it
//! to `codex app-server` as `-c hooks=…`). Codex 0.162.1 (rust-v0.162.1), read from its source:
//!
//! - A command hook has no exec form: `HookHandlerConfig::Command` holds `command`,
//!   `commandWindows`, `timeout`, `async`, `statusMessage` and `additionalContextLimit`
//!   (`config/src/hook_config.rs`). `command` runs as `$SHELL -lc <command>` (else `/bin/sh`), and
//!   on Windows `commandWindows` runs as `%COMSPEC% /C "<command>"` (else `cmd.exe`)
//!   (`hooks/src/engine/command_runner.rs`, `build_command`, `default_shell_program`). So the
//!   program is quoted: POSIX single quotes for `command`, double quotes for `commandWindows`.
//! - The hook passes no `--matter`: Codex's hook input `cwd` is the turn's working folder
//!   (`core/src/hook_runtime.rs`, `tool_hook_cwd`: the local environment's cwd, else the turn's),
//!   which a command's `cd` does not move, so the guard takes the matter from it, and the value
//!   stays one per config. The guard refuses with exit 2 and the reason on stderr, which Codex
//!   blocks on (`hooks/src/events/pre_tool_use.rs`, `Some(2)`), whatever a login shell prints to
//!   stdout.
//! - A hook from session flags runs only when trusted. The value trusts itself with a `state`
//!   entry under the hook's key, `<session-flags layer path>:pre_tool_use:0:0`, whose
//!   `trusted_hash` is the hash Codex computes for the hook (`hooks/src/engine/discovery.rs`,
//!   `hook_hash`; `hooks/src/lib.rs`, `hook_key`). The layer path is
//!   `/<session-flags>/config.toml`, and `C:\<session-flags>\config.toml` on Windows
//!   (`synthetic_layer_path`, normalised by `utils/absolute-path`).
//! - The hash ([`hook_hash`]): Codex serialises the normalised hook identity, `{event_name,
//!   matcher, hooks: [handler]}` with the handler's `command` taken from `commandWindows` on
//!   Windows, no `commandWindows`, `timeout` and `async` filled in and unset options left out, to
//!   TOML, then to JSON with every object's keys sorted, compact, and hashes those bytes with
//!   SHA-256: `sha256:<hex>` (`config/src/fingerprint.rs`, `version_for_toml`). The tests pin
//!   [`fingerprint`] to Codex's own fingerprint test and the hook hashes to a reference run of
//!   Codex's own types and crates.

use std::path::Path;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

/// The tools Codex's guard hook runs before: `apply_patch` (and its aliases `Write` and `Edit`)
/// and `Bash`. Names joined by `|` are an exact list in Codex.
pub(crate) const CODEX_GUARD_MATCHER: &str = "apply_patch|Write|Edit|Bash";

/// The seconds Codex waits for the guard (its default is 600); a timeout lets the tool go on.
const CODEX_GUARD_TIMEOUT_SECONDS: u64 = 10;

/// The key of the first handler of the first `PreToolUse` group from session flags.
#[cfg(not(windows))]
const SESSION_FLAGS_KEY: &str = "/<session-flags>/config.toml:pre_tool_use:0:0";
#[cfg(windows)]
const SESSION_FLAGS_KEY: &str = r"C:\<session-flags>\config.toml:pre_tool_use:0:0";

/// The `hooks` value for Codex's config that runs the pre-write guard, `program` with
/// [`FLAG`](crate::pre_write_guard::FLAG), before each `apply_patch` and `Bash`, trusted by its
/// own `state` entry:
///
/// ```json
/// {"PreToolUse": [{"matcher": "apply_patch|Write|Edit|Bash", "hooks": [{"type": "command",
///   "command": "'<program>' --pre-write-guard",
///   "commandWindows": "\"<program>\" --pre-write-guard", "timeout": 10}]}],
///  "state": {"/<session-flags>/config.toml:pre_tool_use:0:0": {"trusted_hash": "sha256:…"}}}
/// ```
///
/// The hash is for the OS this runs on, as Codex computes it on the same machine. `None` when
/// `program` is no UTF-8 string, or (Windows) holds a `"` or `%`, which `cmd.exe` cannot take
/// inside quotes: no hook rather than one that runs something else. The codex hooks seam
/// (`actor::turn::agents`, `codex_hooks`) puts it in each Codex launch's `CODEX_CONFIG`.
pub fn codex_hook_config(program: &Path) -> Option<Value> {
    let program = program.to_str()?;
    let command = format!("{} {}", posix_quoted(program), crate::pre_write_guard::FLAG);
    let command_windows = format!(
        "{} {}",
        windows_quoted(program)?,
        crate::pre_write_guard::FLAG
    );
    let effective = if cfg!(windows) {
        &command_windows
    } else {
        &command
    };
    let trusted_hash = hook_hash(
        "pre_tool_use",
        CODEX_GUARD_MATCHER,
        effective,
        CODEX_GUARD_TIMEOUT_SECONDS,
    );
    Some(json!({
        "PreToolUse": [{
            "matcher": CODEX_GUARD_MATCHER,
            "hooks": [{
                "type": "command",
                "command": command,
                "commandWindows": command_windows,
                "timeout": CODEX_GUARD_TIMEOUT_SECONDS,
            }],
        }],
        "state": { SESSION_FLAGS_KEY: { "trusted_hash": trusted_hash } },
    }))
}

/// `text` in POSIX single quotes, each `'` as `'\''`. Also right for zsh and bash as `$SHELL`;
/// fish reads `\\` and `\'` inside single quotes as escapes, so a path holding a backslash
/// would change there.
pub(crate) fn posix_quoted(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// `text` in double quotes for `cmd.exe`, or `None` when it holds `"` (no Windows path can) or
/// `%` (which `cmd` expands even inside quotes).
fn windows_quoted(text: &str) -> Option<String> {
    (!text.contains(['"', '%'])).then(|| format!("\"{text}\""))
}

/// The trust hash Codex 0.162.1 computes for a command hook: the identity's JSON, every object's
/// keys sorted, hashed by [`fingerprint`].
pub(crate) fn hook_hash(event: &str, matcher: &str, command: &str, timeout: u64) -> String {
    fingerprint(json!({
        "event_name": event,
        "matcher": matcher,
        "hooks": [{
            "type": "command",
            "command": command,
            "timeout": timeout,
            "async": false,
        }],
    }))
}

/// Codex's `version_for_toml` on the JSON form of a TOML value: keys sorted at every level,
/// compact JSON, SHA-256, `sha256:` and lowercase hex.
pub(crate) fn fingerprint(mut value: Value) -> String {
    value.sort_all_objects();
    let bytes = serde_json::to_vec(&value).unwrap_or_default();
    let digest = Sha256::digest(&bytes);
    let mut hex = String::with_capacity(7 + 64);
    hex.push_str("sha256:");
    for byte in digest {
        hex.push(char::from_digit(u32::from(byte >> 4), 16).unwrap_or('0'));
        hex.push(char::from_digit(u32::from(byte & 0xf), 16).unwrap_or('0'));
    }
    hex
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use serde_json::json;

    use super::{codex_hook_config, fingerprint, hook_hash, posix_quoted};

    /// Codex's own test, `config/src/fingerprint_tests.rs` at rust-v0.162.1: the TOML
    /// `rows = [{ z = 2, a = 1 }, { z = 4, a = 3 }]` with `[nested] z = "text"`, `a = true`.
    #[test]
    fn the_fingerprint_matches_codexs_own_test() {
        let value = json!({
            "rows": [{"z": 2, "a": 1}, {"z": 4, "a": 3}],
            "nested": {"z": "text", "a": true},
        });
        assert_eq!(
            fingerprint(value),
            "sha256:a5e4c8fbdbc75d185b0f2b5291bfb2de10e8c523d6c89248876fd78a97c83176"
        );
    }

    /// Hashes from a reference run of Codex 0.162.1's own `MatcherGroup`, `HookHandlerConfig`,
    /// `NormalizedHookIdentity` and `version_for_toml`, with its serde, toml 0.9.11, `serde_json`
    /// and sha2 0.10.9 (the program is in the PR's notes). The second has a `'` and a space.
    #[test]
    fn hook_hashes_match_codexs_types() {
        let matcher = "apply_patch|Write|Edit|Bash";
        for (command, hash) in [
            (
                "'/opt/clauseo/clauseo-desktop' --pre-write-guard",
                "sha256:57fd0a13a8285ab7294f320ab698cbfcdfdb55d2cad5482964b9fa10551f7f93",
            ),
            (
                r"'/Applications/Clauseo.app/Contents/MacOS/O'\''Brien desktop' --pre-write-guard",
                "sha256:c18fe9dbea6550e031a0d388d276c2e0994ab50c69e27970ea0651f7f4376ebf",
            ),
            (
                r#""C:\Program Files\Clauseo\clauseo-desktop.exe" --pre-write-guard"#,
                "sha256:73ba7cb922498ad47c224263f6cf8c64ea6788f740996b1441621e38d42be62e",
            ),
        ] {
            assert_eq!(
                hook_hash("pre_tool_use", matcher, command, 10),
                hash,
                "{command}"
            );
        }
    }

    #[test]
    fn the_config_runs_the_guard_and_trusts_itself() {
        let config = codex_hook_config(Path::new("/opt/clauseo/clauseo-desktop")).unwrap();
        let (key, hash) = if cfg!(windows) {
            (
                r"C:\<session-flags>\config.toml:pre_tool_use:0:0",
                hook_hash(
                    "pre_tool_use",
                    "apply_patch|Write|Edit|Bash",
                    "\"/opt/clauseo/clauseo-desktop\" --pre-write-guard",
                    10,
                ),
            )
        } else {
            (
                "/<session-flags>/config.toml:pre_tool_use:0:0",
                "sha256:57fd0a13a8285ab7294f320ab698cbfcdfdb55d2cad5482964b9fa10551f7f93"
                    .to_owned(),
            )
        };
        assert_eq!(
            config,
            json!({
                "PreToolUse": [{
                    "matcher": "apply_patch|Write|Edit|Bash",
                    "hooks": [{
                        "type": "command",
                        "command": "'/opt/clauseo/clauseo-desktop' --pre-write-guard",
                        "commandWindows": "\"/opt/clauseo/clauseo-desktop\" --pre-write-guard",
                        "timeout": 10,
                    }],
                }],
                "state": {key: {"trusted_hash": hash}},
            })
        );
    }

    #[test]
    fn quoting_keeps_the_path_one_word_and_refuses_what_cmd_would_expand() {
        assert_eq!(posix_quoted("/a b/O'Brien"), r"'/a b/O'\''Brien'");
        assert_eq!(codex_hook_config(Path::new(r"C:\100%\clauseo.exe")), None);
    }
}
