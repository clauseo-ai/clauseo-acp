use super::*;

/// claude-agent-acp 0.89.1's stderr up to a refused `session/new`, as the process crate's tail
/// gives it (the home folder already `~`).
const TAIL: &str = "\
[session/create] sessionId=7da90d5d-1c5c-4b16-92f4-7ee338481cd8 phase=validate-cwd durationMs=1 totalMs=1
[session/create] sessionId=7da90d5d-1c5c-4b16-92f4-7ee338481cd8 phase=settings durationMs=4 totalMs=5
(node:1354020) [CLAUDE_SDK_CAN_USE_TOOL_SHADOWED] Warning: canUseTool will not be invoked for: WebSearch.
[session/query] sessionId=7da90d5d-1c5c-4b16-92f4-7ee338481cd8 resume=none apiType=native baseUrl=native
[session/create] sessionId=7da90d5d-1c5c-4b16-92f4-7ee338481cd8 phase=prepare-query durationMs=11 totalMs=16
Error: Claude Code process exited with code 1
";

#[test]
fn the_summary_keeps_the_phases_and_the_last_line() {
    assert_eq!(
        start_summary(TAIL),
        "[session/create] sessionId=7da90d5d-1c5c-4b16-92f4-7ee338481cd8 phase=validate-cwd \
         durationMs=1 totalMs=1 | [session/create] sessionId=7da90d5d-1c5c-4b16-92f4-7ee338481cd8 \
         phase=settings durationMs=4 totalMs=5 | [session/create] \
         sessionId=7da90d5d-1c5c-4b16-92f4-7ee338481cd8 phase=prepare-query durationMs=11 \
         totalMs=16 | Error: Claude Code process exited with code 1"
    );
}

#[test]
fn a_phase_last_line_goes_once_and_an_empty_tail_is_none() {
    let tail = "noise\n[session/create] sessionId=s phase=settings durationMs=4 totalMs=5\n\n";
    assert_eq!(
        start_summary(tail),
        "[session/create] sessionId=s phase=settings durationMs=4 totalMs=5"
    );
    assert_eq!(start_summary(""), "none");
    assert_eq!(start_summary("\n  \n"), "none");
}

#[test]
fn the_summary_is_cut_on_a_character_boundary() {
    let long = format!("{}é", "x".repeat(START_TAIL_BYTES - 1));
    let summary = start_summary(&long);
    assert_eq!(summary.len(), START_TAIL_BYTES - 1);
    assert!(summary.chars().all(|c| c == 'x'));
}
