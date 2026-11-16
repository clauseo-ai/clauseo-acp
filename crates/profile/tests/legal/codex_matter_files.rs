//! The line that tells Codex a matter's `AGENTS.md`, `AGENTS.override.md` and `SKILL.md` files
//! are documents, never instructions: every Codex session gets it, short, and Claude never.

use clauseo_profile::instructions::{CODEX_MATTER_FILES, word_count};
use clauseo_protocol::AgentKind;

/// One sentence of at most 20 words naming the three file names, in every Codex session's
/// instructions and keys, on macOS and elsewhere, and in no Claude session's.
#[test]
fn codex_is_told_a_matters_agent_files_are_documents() {
    let line = CODEX_MATTER_FILES.trim();
    assert!(!line.contains('\n'));
    assert_eq!(line.matches(". ").count(), 0, "one sentence: {line}");
    assert!(word_count(line) <= 20, "{} words: {line}", word_count(line));
    for name in ["AGENTS.md", "AGENTS.override.md", "SKILL.md"] {
        assert!(line.contains(name), "{name}");
    }
    let profile = super::launch_profile(&std::env::temp_dir().join("codex-matter-files"));
    for macos in [true, false] {
        assert!(profile.codex_instructions_for(macos).contains(line));
        assert!(!profile.claude_instructions_for(macos).contains(line));
    }
    let keys = profile.codex_keys_without_index(Some(AgentKind::Codex));
    let text = keys["developer_instructions"].as_str().unwrap();
    assert!(text.contains(line));
}
