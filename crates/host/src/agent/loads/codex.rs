//! The pre-write guard's hook for Codex, for R15 to merge into `CODEX_CONFIG` (codex-acp passes it
//! to `codex app-server` as `-c hooks=…`). Codex 0.162.1 (rust-v0.162.1), read from its source:
//!
//! - A command hook has no exec form: `HookHandlerConfig::Command` holds `command`,
//!   `commandWindows`, `timeout`, `async`, `statusMessage` and `additionalContextLimit`
//!   (`config/src/hook_config.rs`). The hook runs under the session's own shell, its exec form
//!   without the last word (`core/src/session/mod.rs:5141-5149, 5164`, the local environment's
//!   shell): on macOS and Linux the user's `zsh` or `bash` with `-c <command>`, and on Windows
//!   PowerShell, `powershell.exe -NoProfile -Command <commandWindows>`
//!   (`shell-command/src/shell_detect.rs:350-353`); `%COMSPEC% /C "<command>"` only when the
//!   session has no shell (`hooks/src/engine/command_runner.rs:384-409`). So `command` quotes in
//!   POSIX single quotes, and `commandWindows` is one PowerShell statement ([`ps_command`]).
//!   On Windows the line names the console program [`GUARD_EXE`] beside the app
//!   ([`codex_guard_program`]): PowerShell waits for a console program and reads its exit code.
//!   Under `cmd`, the session's fallback shell, this line does not run as one command; where
//!   PowerShell is blocked or absent, Codex can't start the guard.
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

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

/// The tools Codex's guard hook runs before: `apply_patch` (and its aliases `Write` and `Edit`)
/// and `Bash`. Names joined by `|` are an exact list in Codex.
pub(crate) const CODEX_GUARD_MATCHER: &str = "apply_patch|Write|Edit|Bash";

/// The seconds Codex waits for the guard (its default is 600), room for a cold PowerShell start
/// and a first scan of a new program.
const CODEX_GUARD_TIMEOUT_SECONDS: u64 = 30;

/// The key of the first handler of the first `PreToolUse` group from session flags.
#[cfg(not(windows))]
const SESSION_FLAGS_KEY: &str = "/<session-flags>/config.toml:pre_tool_use:0:0";
#[cfg(windows)]
const SESSION_FLAGS_KEY: &str = r"C:\<session-flags>\config.toml:pre_tool_use:0:0";

/// The `hooks` value for Codex's config that runs the pre-write guard, `program` with
/// [`FLAG`](crate::pre_write_guard::FLAG) and, with `record`, [`RECORD_FLAG`] and that file
/// (the session's `guard-blocks.jsonl`, `actor/turn/updates/guard_blocks.rs`) then
/// [`RECORD_ID_FLAG`] and its identity (hex digits, `pre_write_guard::make_record`), before each
/// `apply_patch` and `Bash`, trusted by its own `state` entry:
///
/// ```json
/// {"PreToolUse": [{"matcher": "apply_patch|Write|Edit|Bash", "hooks": [{"type": "command",
///   "command": "'<program>' --pre-write-guard --record '<record>' --record-id <hex>",
///   "commandWindows": "$global:LASTEXITCODE = -1; & '<program>' --pre-write-guard --record
///     '<record>' --record-id '<hex>'; $c = $LASTEXITCODE; if ($c -eq 0 -or $c -eq 2) { exit $c };
///     [Console]::Error.WriteLine('…'); exit 2", "timeout": 30}]}],
///  "state": {"/<session-flags>/config.toml:pre_tool_use:0:0": {"trusted_hash": "sha256:…"}}}
/// ```
///
/// The hash is for the OS this runs on, as Codex computes it on the same machine, so it changes
/// with the record file, one per session. `None` when `program` is no UTF-8 string: no hook
/// rather than one that runs something else. A record file with no UTF-8 spelling, or an
/// identity of anything but hex digits, goes out of the command alone: the guard still runs, and
/// the lawyer is not told of its refusals. The codex hooks seam (`actor::turn::agents`,
/// `codex_hooks`) puts it in each Codex launch's `CODEX_CONFIG`.
///
/// [`RECORD_FLAG`]: crate::pre_write_guard::RECORD_FLAG
/// [`RECORD_ID_FLAG`]: crate::pre_write_guard::RECORD_ID_FLAG
pub fn codex_hook_config(program: &Path, record: Option<(&Path, &str)>) -> Option<Value> {
    let program = program.to_str()?;
    let hex = |id: &&str| !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_hexdigit());
    let record = record.and_then(|(file, id)| Some((file.to_str()?, Some(id).filter(hex)?)));
    let (flag, id_flag) = (
        crate::pre_write_guard::RECORD_FLAG,
        crate::pre_write_guard::RECORD_ID_FLAG,
    );
    let mut command = format!("{} {}", posix_quoted(program), crate::pre_write_guard::FLAG);
    let mut windows = vec![ps_quoted(program), crate::pre_write_guard::FLAG.to_owned()];
    if let Some((file, id)) = record {
        command = format!("{command} {flag} {} {id_flag} {id}", posix_quoted(file));
        windows.extend([
            flag.to_owned(),
            ps_quoted(file),
            id_flag.to_owned(),
            ps_quoted(id),
        ]);
    }
    let command_windows = ps_command(&windows);
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

pub(crate) use crate::pre_write_guard::UNCHECKED;

/// The console program Codex's hook runs on Windows, installed beside the app.
pub const GUARD_EXE: &str = "clauseo-guard.exe";

/// The program Codex's hook line names for the app at `app`: on Windows [`GUARD_EXE`] in the
/// app's folder, a console program, which PowerShell waits for and whose exit code it reads (the
/// app is a GUI program in a release build); elsewhere the app itself. With [`codex_hook_config`]
/// this is the one way to build the line, for the hook and for a check that starts the guard as
/// Codex does. A missing program fails closed: the line ends with 2 and [`UNCHECKED`].
///
/// Scope: the program sits in the app's install folder beside `Clauseo.exe` (per user, under
/// `%LOCALAPPDATA%`; `tauri.conf.json` `installMode`), under the same rules as the app: no fence
/// of Clauseo's names that folder, and what stands between Codex's shell and either program is
/// Codex's own sandbox (the ask mode with Windows' unelevated sandbox, `agent/codex.rs`
/// `ASK_MODE`, `WINDOWS_SANDBOX`), which asks before a write outside the matter. A firm whose
/// allow rule names only `Clauseo.exe` by path refuses the program, and every Codex step is then
/// refused.
pub fn codex_guard_program(app: &Path) -> PathBuf {
    if cfg!(windows) {
        app.with_file_name(GUARD_EXE)
    } else {
        app.to_path_buf()
    }
}

/// `words` (the program, then its arguments, each quoted as it must be) as one line of
/// PowerShell: the call operator runs the program, whose stdin, stdout and stderr are the hook's
/// own; its exit code 0 (go on) or 2 (refused, the reason on its stderr) ends the hook with that
/// same code. `$LASTEXITCODE` is set to -1 first, so a program that never started can't leave an
/// unset or stale code. Any other ending (no program at that path, a crash, 1, an NTSTATUS)
/// writes [`UNCHECKED`] to stderr and ends with 2, so Codex blocks the tool with that line as its
/// reason (`hooks/src/events/pre_tool_use.rs:261-291`).
pub(crate) fn ps_command(words: &[String]) -> String {
    format!(
        "$global:LASTEXITCODE = -1; & {}; $c = $LASTEXITCODE; if ($c -eq 0 -or $c -eq 2) \
         {{ exit $c }}; [Console]::Error.WriteLine({}); exit 2",
        words.join(" "),
        ps_quoted(UNCHECKED),
    )
}

/// `text` in PowerShell single quotes, each `'` doubled (the typographic quotes PowerShell also
/// reads as quotes too): nothing inside expands, not `$`, a backtick or `%`.
pub(crate) fn ps_quoted(text: &str) -> String {
    let mut quoted = String::with_capacity(text.len() + 2);
    quoted.push('\'');
    for ch in text.chars() {
        if matches!(ch, '\'' | '\u{2018}' | '\u{2019}' | '\u{201a}' | '\u{201b}') {
            quoted.push(ch);
        }
        quoted.push(ch);
    }
    quoted.push('\'');
    quoted
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

    use super::{codex_hook_config, fingerprint, hook_hash, posix_quoted, ps_command, ps_quoted};

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
        let config = codex_hook_config(Path::new("/opt/clauseo/clauseo-desktop"), None).unwrap();
        let windows = ps_command(&[
            "'/opt/clauseo/clauseo-desktop'".to_owned(),
            "--pre-write-guard".to_owned(),
        ]);
        let (key, hash) = if cfg!(windows) {
            (
                r"C:\<session-flags>\config.toml:pre_tool_use:0:0",
                hook_hash("pre_tool_use", "apply_patch|Write|Edit|Bash", &windows, 30),
            )
        } else {
            (
                "/<session-flags>/config.toml:pre_tool_use:0:0",
                "sha256:1674028cddd3b0a1ede5c83351d6fa14f67d1c995b607b04ad58aab21ffb4624"
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
                        "commandWindows": windows,
                        "timeout": 30,
                    }],
                }],
                "state": {key: {"trusted_hash": hash}},
            })
        );
    }

    /// Each path stays one word for its shell; PowerShell's single quotes expand nothing, so a
    /// `%`, a `$` or a backtick in a Windows path goes in as it is.
    #[test]
    fn quoting_keeps_the_path_one_word() {
        assert_eq!(posix_quoted("/a b/O'Brien"), r"'/a b/O'\''Brien'");
        assert_eq!(
            ps_quoted(r"C:\O'Brien & Sons; $x `y 100% (2024)"),
            r"'C:\O''Brien & Sons; $x `y 100% (2024)'"
        );
        assert_eq!(ps_quoted("C:\u{2019}x"), "'C:\u{2019}\u{2019}x'");
        let config = codex_hook_config(Path::new(r"C:\100%\clauseo.exe"), None).unwrap();
        assert!(
            config["PreToolUse"][0]["hooks"][0]["commandWindows"]
                .as_str()
                .unwrap()
                .starts_with(
                    r"$global:LASTEXITCODE = -1; & 'C:\100%\clauseo.exe' --pre-write-guard; $c"
                )
        );
    }

    /// The Windows line runs the program with PowerShell's call operator and ends with the
    /// guard's code when that is 0 or 2, else writes the plain words to stderr and ends with 2
    /// (run by PowerShell on Windows CI: `codex/windows_tests.rs`).
    #[test]
    fn the_windows_line_is_one_powershell_statement_that_blocks_unless_the_guard_answers() {
        let line = ps_command(&[
            "'C:\\a b\\clauseo.exe'".to_owned(),
            "--pre-write-guard".to_owned(),
        ]);
        assert_eq!(
            line,
            "$global:LASTEXITCODE = -1; & 'C:\\a b\\clauseo.exe' --pre-write-guard; $c = \
             $LASTEXITCODE; if ($c -eq 0 -or $c -eq 2) { exit $c }; [Console]::Error.WriteLine('Clauseo \
             could not check this step, so it did not run.'); exit 2"
        );
        assert!(!line.contains('\n'));
    }

    /// The session's record file goes after the flag, quoted for each shell, then its identity,
    /// and the trust hash is the one Codex computes for that whole command; an identity of
    /// anything but hex digits leaves both commands without them.
    #[test]
    fn the_record_file_and_its_identity_go_in_the_command_and_its_hash() {
        let program = Path::new("/opt/clauseo/clauseo-desktop");
        let record = Path::new("/data/browser/guard/s 1/guard-blocks.jsonl");
        let id = "00000000000001030000000000000000000000000045c2a9";
        let config = codex_hook_config(program, Some((record, id))).unwrap();
        let hook = &config["PreToolUse"][0]["hooks"][0];
        let posix = format!(
            "'/opt/clauseo/clauseo-desktop' --pre-write-guard --record \
             '/data/browser/guard/s 1/guard-blocks.jsonl' --record-id {id}"
        );
        let windows = ps_command(&[
            "'/opt/clauseo/clauseo-desktop'".to_owned(),
            "--pre-write-guard".to_owned(),
            "--record".to_owned(),
            "'/data/browser/guard/s 1/guard-blocks.jsonl'".to_owned(),
            "--record-id".to_owned(),
            ps_quoted(id),
        ]);
        assert_eq!(hook["command"], posix);
        assert_eq!(hook["commandWindows"], windows);
        let effective = if cfg!(windows) { &windows } else { &posix };
        let state = config["state"].as_object().unwrap();
        let [(_, entry)] = state.iter().collect::<Vec<_>>()[..] else {
            panic!("one trust entry: {state:?}");
        };
        let trusted = &entry["trusted_hash"];
        assert_eq!(
            *trusted,
            hook_hash("pre_tool_use", "apply_patch|Write|Edit|Bash", effective, 30)
        );
        if !cfg!(windows) {
            assert_eq!(
                *trusted,
                "sha256:a7cad9406971426d55a138ac2b7994ff2291be229fdba66206fc9b69e34ba1b1"
            );
        }
        let odd = Path::new("/data/100%/guard-blocks.jsonl");
        let odd = codex_hook_config(program, Some((odd, id))).unwrap();
        let odd = &odd["PreToolUse"][0]["hooks"][0];
        assert!(
            odd["commandWindows"]
                .as_str()
                .unwrap()
                .contains("--record '/data/100%/guard-blocks.jsonl' --record-id")
        );
        assert!(odd["command"].as_str().unwrap().ends_with(&format!(
            "--record '/data/100%/guard-blocks.jsonl' --record-id {id}"
        )));
        for bad in ["", "12 && calc", "'x'", "00g1"] {
            let config = codex_hook_config(program, Some((record, bad))).unwrap();
            let hook = &config["PreToolUse"][0]["hooks"][0];
            assert_eq!(
                hook["command"], "'/opt/clauseo/clauseo-desktop' --pre-write-guard",
                "{bad:?}"
            );
        }
    }
}

// Two attributes, not `all(test, windows)`: clippy counts a module as test code (unwraps and
// prints allowed, clippy.toml) only under a plain `cfg(test)`.
#[cfg(test)]
#[cfg(windows)]
mod windows_tests;
