//! S15 `PR4a2`: the protected names write-denied at any depth of the matter.

use super::*;

/// S15 `PR4a2`: every `.claude`, `.mcp.json`, `CLAUDE.md` and `AGENTS.md` at any depth, in any
/// letter case, is write-denied by a subpath rule of its own, so the inside of a `.claude` too.
/// One inside a folder already denied gets no rule of its own; other names get none.
#[test]
fn every_protected_name_at_any_depth_is_read_only() {
    let w = World::new("nested-depth");
    for rel in [
        "a/CLAUDE.md",
        "a/b/c/d/claude.MD",
        "x/AGENTS.md",
        "x/y/Agents.md",
        "p/.claude/settings.json",
        "p/q/.Claude/hooks.json",
        "m/.mcp.json",
        "m/n/.MCP.json",
        // Inside a folder denied already.
        "p/.claude/CLAUDE.md",
        ".codex/AGENTS.md",
        "r/.git/AGENTS.md",
        // Not protected names.
        "a/CLAUDE.md.bak",
        "a/b/CLAUDE.local.md",
        "a/notes.md",
    ] {
        w.dir.file(&format!("matter/{rel}"), "x");
    }
    let policy = w.builder().build().unwrap();
    let matter = canon(&w.at("matter"));
    let mut below: Vec<&Path> = policy
        .read_only_in_matter()
        .iter()
        .filter_map(|rule| match rule {
            PathRule::Subpath(p) => p.strip_prefix(&matter).ok(),
            PathRule::Literal(_) => None,
        })
        .filter(|p| p.components().count() > 1)
        .collect();
    below.sort();
    let want = [
        "a/CLAUDE.md",
        "a/b/c/d/claude.MD",
        "m/.mcp.json",
        "m/n/.MCP.json",
        "p/.claude",
        "p/q/.Claude",
        "r/.git",
        "x/AGENTS.md",
        "x/y/Agents.md",
    ];
    assert_eq!(below, want.map(Path::new));
    for rel in ["p/q/.Claude/hooks.json", "x/y/Agents.md", "m/n/.MCP.json"] {
        assert_eq!(
            policy.classify(&matter.join(rel), Access::Write),
            PathClass::ReadOnly,
            "{rel}"
        );
    }
}
