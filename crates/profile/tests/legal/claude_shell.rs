//! The line Claude on macOS gets about the commands its sandbox blocks.

/// Claude Code's own sandbox (the host's `agent::loads::sandbox`) runs Claude's commands on
/// macOS under Seatbelt with Apple events off and no way out, so the same services fail for it:
/// Claude on macOS is told, in one sentence of its own; Claude elsewhere (Windows has no sandbox
/// for it) and Codex never are.
#[test]
fn only_claude_on_macos_is_told_which_commands_its_sandbox_blocks() {
    use clauseo_profile::instructions::{claude_shell, codex_shell};
    use clauseo_protocol::AgentKind;
    let line = claude_shell(true).unwrap();
    assert_eq!(claude_shell(false), None);
    assert_ne!(Some(line), codex_shell(true));
    assert_eq!(line.matches(". ").count(), 0, "one sentence: {line}");
    for command in [
        "mdls",
        "mdfind",
        "open",
        "qlmanage",
        "osascript",
        "stetdoc",
        "textutil",
        "python3",
    ] {
        assert!(line.contains(&format!("`{command}`")), "{command}");
    }
    let profile = super::launch_profile(&std::env::temp_dir().join("claude-shell-line"));
    assert_eq!(
        profile.claude_instructions_for(true),
        format!("{}\n\n{line}", profile.instructions())
    );
    assert_eq!(
        profile.claude_instructions_for(false),
        profile.instructions()
    );
    let meta = profile.session_meta(Some(AgentKind::Claude)).unwrap();
    assert_eq!(
        meta["systemPrompt"]["append"],
        profile.claude_instructions().as_str()
    );
    assert_eq!(
        profile.claude_instructions().contains(line),
        cfg!(target_os = "macos")
    );
    assert!(!profile.instructions().contains(line));
    assert!(!profile.codex_instructions_for(true).contains(line));
    assert!(profile.session_meta(Some(AgentKind::Codex)).is_none());
}
