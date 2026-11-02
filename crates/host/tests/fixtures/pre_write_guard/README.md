# Pre-write guard input fixtures

One `PreToolUse` hook input per tool shape the guard reads, as the agent CLI writes it to the
guard's stdin. `src/pre_write_guard/tests/fixtures.rs` replaces `{matter}` with a test matter
folder that holds `Contract.docx` (open in Word: its owner file `~$ntract.docx` beside it) and
`Free.docx`, and checks that the guard reads each one: refused as it stands, let through with
`Contract.docx` changed to `Free.docx`, and never the "ignored its input" line. When an agent
update changes a shape, that test fails instead of the guard silently letting writes through
(it fails open on input it cannot read, clauseo-acp-private#162).

| Folder | Agent | Version the shapes match | Source |
|---|---|---|---|
| `claude/` | Claude Code | CLI 2.1.293, bundled with claude-agent-acp 0.89.1 (the pinned adapter); the `Write` and `Bash` shapes also replayed against CLI 2.1.316 in #162's live check | Hook input fields `session_id`, `transcript_path`, `cwd`, `permission_mode`, `hook_event_name`, `tool_name`, `tool_input`, `tool_use_id` (the CLI's hook schema); each tool's `tool_input` is the tool's own input schema |
| `codex/` | Codex | `@openai/codex` 0.162.1 (rust-v0.162.1), under codex-acp 2.2.2 | `hooks/src/events/pre_tool_use.rs`, `command_input_json` (fields `session_id`, `turn_id`, `transcript_path`, `cwd`, `hook_event_name`, `model`, `permission_mode`, `tool_name`, `tool_input`, `tool_use_id`); `apply_patch` and `Bash` send `{"command": "<text>"}` (`core/src/hook_runtime.rs`) |

The identifiers (`session_id`, `tool_use_id`, paths) are made up. When a pinned agent version
changes, update the shapes and this table together.
