//! The agents' `PreToolUse` input shapes as captured for the pinned versions
//! (`tests/fixtures/pre_write_guard`, README.md there names them and the rule to capture them
//! again when a version moves): the guard reads each one, not through its fail-open path.

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

/// The words refusing any checked tool in a mode that skips the lawyer's approval.
const UNASKED: &str = "Clauseo doesn't let the agent change files in a permission mode that skips \
                       the lawyer's approval. Wait until Clauseo has put the session back in its \
                       own mode.\n";

/// Each Claude Code fixture on a file nobody holds: in `bypassPermissions` and `dontAsk` the
/// tool is refused with the mode's reason; in `acceptEdits`, `auto`, `plan` and `default` it
/// goes on.
#[test]
fn claude_tools_are_refused_in_a_mode_that_skips_the_lawyer() {
    let fx = Fixture::new("fixtures-modes");
    let matter = fx.path("m");
    let escaped = serde_json::to_string(&matter).unwrap();
    let escaped = escaped.trim_matches('"');
    for (name, text, _) in FIXTURES.into_iter().filter(|(_, _, claude)| *claude) {
        let free = text
            .replace("{matter}", escaped)
            .replace("Contract.docx", "Free.docx");
        let seen = "\"permission_mode\": \"acceptEdits\"";
        assert!(free.contains(seen), "{name} has no acceptEdits mode");
        for (mode, refused) in [
            ("bypassPermissions", true),
            ("dontAsk", true),
            ("acceptEdits", false),
            ("auto", false),
            ("plan", false),
            ("default", false),
        ] {
            let input = free.replace(seen, &format!("\"permission_mode\": \"{mode}\""));
            let said = guard_with(&["--matter", &matter], input.as_bytes());
            let want = if refused { UNASKED } else { "" };
            assert_eq!(said, want, "{name} in {mode}");
        }
    }
}
