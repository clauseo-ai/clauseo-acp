//! The agents' current `PreToolUse` input shapes (`tests/fixtures/pre_write_guard`, README.md
//! there names the versions): the guard reads each one, so a shape change fails here instead of
//! the guard failing open in the field.

use super::{Fixture, IGNORED, deny, guard, guard_with};

/// Each fixture: its file, its text, and whether the agent's hook passes `--matter` (Claude
/// Code's does, Codex's does not).
const FIXTURES: [(&str, &str, bool); 8] = [
    (
        "claude/Write.json",
        include_str!("../../../tests/fixtures/pre_write_guard/claude/Write.json"),
        true,
    ),
    (
        "claude/Edit.json",
        include_str!("../../../tests/fixtures/pre_write_guard/claude/Edit.json"),
        true,
    ),
    (
        "claude/MultiEdit.json",
        include_str!("../../../tests/fixtures/pre_write_guard/claude/MultiEdit.json"),
        true,
    ),
    (
        "claude/NotebookEdit.json",
        include_str!("../../../tests/fixtures/pre_write_guard/claude/NotebookEdit.json"),
        true,
    ),
    (
        "claude/Bash.json",
        include_str!("../../../tests/fixtures/pre_write_guard/claude/Bash.json"),
        true,
    ),
    (
        "claude/PowerShell.json",
        include_str!("../../../tests/fixtures/pre_write_guard/claude/PowerShell.json"),
        true,
    ),
    (
        "codex/apply_patch.json",
        include_str!("../../../tests/fixtures/pre_write_guard/codex/apply_patch.json"),
        false,
    ),
    (
        "codex/Bash.json",
        include_str!("../../../tests/fixtures/pre_write_guard/codex/Bash.json"),
        false,
    ),
];

#[test]
fn the_guard_reads_each_current_agent_input_shape() {
    let fx = Fixture::new("fixtures");
    let matter = fx.path("m");
    // The folder as a JSON string's content: Windows' backslashes escaped.
    let escaped = serde_json::to_string(&matter).unwrap();
    let escaped = escaped.trim_matches('"');
    for (name, text, with_matter) in FIXTURES {
        let held = text.replace("{matter}", escaped);
        assert_ne!(held, text, "{name} names no {{matter}}");
        let free = held.replace("Contract.docx", "Free.docx");
        let check = |input: &str| {
            if with_matter {
                guard_with(&["--matter", &matter], input.as_bytes())
            } else {
                guard(input.as_bytes())
            }
        };
        assert_eq!(check(&held), deny("Contract.docx"), "{name}");
        let let_through = check(&free);
        assert!(!let_through.contains(IGNORED), "{name}: {let_through}");
        assert_eq!(let_through, "", "{name}");
    }
}
