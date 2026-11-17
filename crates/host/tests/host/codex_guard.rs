//! A Codex agent and the guard's trust check: a hooks object goes through `CODEX_CONFIG` only, the
//! launch carries the real guard hook, an untrusted hook fails before any prompt, and without a
//! hooks object the check is skipped.

use std::sync::Arc;

use clauseo_protocol::{ErrorCode, StopReason};
use serde_json::{Map, Value, json};

use crate::codex::{
    CODEX, HANDSHAKE_TEXT, Recording, assert_failed_before_prompt, codex_dir, one_turn, var,
};

/// A small hooks object, as the codex hooks seam would give.
fn hooks() -> Map<String, Value> {
    json!({"PreToolUse": [{"hooks": [{"type": "command", "command": "/bin/guard"}]}]})
        .as_object()
        .cloned()
        .unwrap()
}

#[test]
fn codex_guard_trusted_runs_the_turn() {
    let dir = codex_dir("codex_hook_trusted", "codex-hook-trusted");
    let host = dir.start_with(0, |config| config.with_codex_hooks(hooks()));
    let (_, events, turn) = one_turn(&dir, &host, CODEX);
    events.assert_no_mismatch();
    assert_eq!(events.turn_ended(turn).unwrap().stop, StopReason::EndTurn);
    assert_eq!(events.turn_text(turn), "Guarded.");
    // The hooks went through CODEX_CONFIG only: the home's config.toml has no hooks table, or
    // the guard's session-flags trust key would not match.
    let written = std::fs::read_to_string(
        dir.data_dir()
            .join("agents")
            .join(CODEX)
            .join("home")
            .join("config.toml"),
    )
    .unwrap();
    assert!(written.contains("features.plugins = false"), "{written}");
    assert!(!written.contains("hooks"), "{written}");
    host.shutdown();
}

/// With only the guard program set (as the desktop app sets it), the codex hooks seam gives
/// `codex_hook_config`'s real value: `CODEX_CONFIG.hooks.PreToolUse` holds exactly one group, the
/// guard's, and the session start passes the trust check with it.
#[test]
fn codex_launch_carries_the_real_guard_hook_and_passes_the_trust_check() {
    const PROGRAM: &str = "/opt/clauseo/clauseo-desktop";
    let dir = codex_dir("codex_real_guard", "codex-hook-trusted");
    let launcher = Recording::new(&dir);
    let envs = Arc::clone(&launcher.envs);
    let host = dir.start_launching(0, Arc::new(launcher), |config| {
        config.with_pre_write_guard(std::path::PathBuf::from(PROGRAM))
    });
    let (_, events, turn) = one_turn(&dir, &host, CODEX);
    events.assert_no_mismatch();
    assert_eq!(events.turn_ended(turn).unwrap().stop, StopReason::EndTurn);
    assert_eq!(events.turn_text(turn), "Guarded.");
    host.shutdown();

    let envs = envs.lock().unwrap();
    let [codex] = envs.as_slice() else {
        panic!("one launch, not {}", envs.len());
    };
    let config: Value = serde_json::from_str(&var(codex, "CODEX_CONFIG").unwrap()).unwrap();
    let groups = config["hooks"]["PreToolUse"].as_array().unwrap();
    assert_eq!(groups.len(), 1, "{groups:?}");
    assert_eq!(groups[0]["matcher"], "apply_patch|Write|Edit|Bash");
    // The guard records its refusals in a folder of the session's own,
    // `<data>/browser/guard/<session>`, which no agent's shell may write, and writes only the
    // file whose identity follows `--record-id` (`guard_record.rs`).
    let command = groups[0]["hooks"][0]["command"].as_str().unwrap();
    // On Windows the hook names the console program beside the app; Codex's home goes last.
    let program = clauseo_host::codex_guard_program(std::path::Path::new(PROGRAM));
    let shown = program.display();
    let home = dir.data_dir().join("agents").join("codex-acp").join("home");
    let (record, id) = command
        .strip_prefix(&format!("'{shown}' --pre-write-guard --record '"))
        .and_then(|rest| rest.strip_suffix(&format!(" --codex-home '{}'", home.display())))
        .and_then(|rest| rest.split_once("' --record-id "))
        .unwrap_or_else(|| panic!("{command}"));
    let record = std::path::PathBuf::from(record);
    let below = crate::guard_record::below_data(&dir, &record);
    let [browser, guard, session, file] = below.as_slice() else {
        panic!("{below:?}");
    };
    assert_eq!(
        [browser.as_str(), guard.as_str(), file.as_str()],
        ["browser", "guard", "guard-blocks.jsonl"],
        "{command}"
    );
    assert!(!session.is_empty(), "{command}");
    assert_eq!(id.len(), 48, "{command}");
    let hook_for = clauseo_host::codex_hook_config_with_home;
    let expected = hook_for(&program, Some((&record, id)), Some(&home)).unwrap();
    assert_eq!(config["hooks"], expected);
}

#[test]
fn codex_guard_untrusted_fails_before_any_prompt() {
    let dir = codex_dir("codex_hook_untrusted", "codex-hook-untrusted");
    let host = dir.start_with(0, |config| config.with_codex_hooks(hooks()));
    let (_, events, turn) = one_turn(&dir, &host, CODEX);
    assert_failed_before_prompt(&events, turn, ErrorCode::AgentLaunchFailed);
    host.shutdown();
}

/// Without a hooks object the trust check is skipped: `codex-handshake` never expects
/// `_codex/hooks/list`, so a call to it would be a mismatch.
#[test]
fn codex_without_hooks_skips_the_trust_check() {
    let dir = codex_dir("codex_no_hooks", "codex-handshake");
    let host = dir.start(0);
    let (_, events, turn) = one_turn(&dir, &host, CODEX);
    events.assert_no_mismatch();
    assert_eq!(events.turn_ended(turn).unwrap().stop, StopReason::EndTurn);
    assert_eq!(events.turn_text(turn), HANDSHAKE_TEXT);
    host.shutdown();
}
