//! The built-in tools a Claude session is offered (decision record 0030): an explicit list in
//! `claudeCode.options.tools`, never Claude Code's whole `claude_code` preset, and the tools left
//! out of it named again in `disallowedTools`, a second wall.
//!
//! claude-agent-acp 0.89.1 passes `_meta.claudeCode.options.tools` to the SDK as is
//! (`acp-agent.js:7241-7242`), which runs Claude Code 2.1.293 with `--tools <names>`. Claude
//! Code then offers only those built-in tools that the build also enables (a name it does not
//! enable, or does not know, is dropped silently), MCP tools apart. A `disallowedTools` name
//! still takes a tool out of the list. Both were run against the pinned binary offline (the
//! request's `tools`, through `scripts/claude-rule-probe/fakeapi.py`). The list differs by
//! system ([`claude_tools`]): Windows' build has no embedded search, so it ships `Glob` and
//! `Grep`, and its shell may be `PowerShell`.
//!
//! A Claude Code pin bump that adds a built-in tool offers nothing new until someone adds it
//! here; `tools_tests.rs` pins both lists per system.

use serde_json::{Map, Value};

/// The built-in tools every Claude session is offered, on every system, and why:
/// - `Read`, `Edit`, `Write`, `NotebookEdit`: the papers. A background command's output is read
///   with `Read` too (Claude Code names its output file and says so).
/// - `Bash`: the shell (on Windows Git Bash's, where it is installed; the one-shell work denies
///   it there).
/// - `Agent`: Clauseo's helpers (`clauseo_profile::subagents`), and Claude Code's own types.
/// - `SendMessage`: continues a helper, which starts in the background (Claude Code's answer to
///   `Agent` says so). `TaskStop`: stops a background command or helper.
/// - `Skill`: the built-in skills and the launch's packs (decision record 0040).
/// - `EnterPlanMode`, `ExitPlanMode`: plan mode (`desktop/src/bridge.rs` handles a plan exit).
/// - `AskUserQuestion`: claude-agent-acp denies it itself when the window has no form to show it
///   in (`acp-agent.js:7236`).
/// - `ToolSearch`: loads the schema of a tool Claude Code defers (a plan exit loads
///   `ExitPlanMode`'s so on some accounts, live run of 2026-11-05).
/// - `ReportFindings`: the built-in code-review skill's typed answer; it only shows text.
/// - `TodoWrite`, `TaskCreate`, `TaskGet`, `TaskList`, `TaskUpdate`: the to-do list, in whichever
///   form the build has (2.1.293 offered neither in the runs, here or live).
pub(crate) const CLAUDE_TOOLS: [&str; 19] = [
    "Read",
    "Edit",
    "Write",
    "NotebookEdit",
    "Bash",
    "Agent",
    "SendMessage",
    "TaskStop",
    "Skill",
    "EnterPlanMode",
    "ExitPlanMode",
    "AskUserQuestion",
    "ToolSearch",
    "ReportFindings",
    "TodoWrite",
    "TaskCreate",
    "TaskGet",
    "TaskList",
    "TaskUpdate",
];

/// The tools only Windows adds to [`CLAUDE_TOOLS`]: `PowerShell` (on by default for some
/// accounts, and the one shell with the one-shell work), and `Glob` and `Grep`, which Windows'
/// build ships because it has no embedded search behind its shell; macOS's and Linux's builds
/// search through `Bash` and offer neither (the request's `tools`, live and offline).
pub(crate) const WINDOWS_TOOLS: [&str; 3] = ["PowerShell", "Glob", "Grep"];

/// The web's tools, offered only when `tools.web_search` is on (and denied by name when it is
/// off, `profile::WEB_TOOLS`).
pub(crate) const WEB_TOOLS: [&str; 2] = ["WebFetch", "WebSearch"];

/// The built-in tools Claude Code 2.1.293 knows that Clauseo leaves out, each also in
/// `disallowedTools`, and what a lawyer's conversation loses without it:
/// - `CronCreate`, `CronDelete`, `CronList`: prompts scheduled to run later
///   ([`super::CLAUDE_CRON_OFF_VAR`] also turns the scheduler off). Loses nothing: Clauseo has
///   no scheduling, and a lawyer asks when present.
/// - `ScheduleWakeup`: the same for the session itself; the variable leaves it offered, so it is
///   named. Loses nothing, as above.
/// - `RemoteTrigger`: starts work on claude.ai's side. Loses nothing: a conversation runs here.
/// - `EnterWorktree`, `ExitWorktree`: run git and move the session's folder into a worktree,
///   elsewhere with `path`. Loses nothing: a matter folder is no repository to branch.
/// - `Workflow`: starts agents from a script, outside the helpers' limits. Loses nothing:
///   Clauseo's helpers go through `Agent`.
/// - `REPL`: runs code in Claude Code's own interpreter, beside the shell. Loses nothing: the
///   shell runs what a matter needs.
/// - `LSP`: asks a code language server. Loses nothing a matter needs.
/// - `DesignSync`: sends files to and from claude.ai's design projects, off this computer.
///   Loses nothing a matter needs.
/// - `PushNotification`, `SendUserMessage` (`Brief`, the same tool): reach the lawyer outside
///   the conversation. Loses nothing: the window shows each answer and when a turn ends.
/// - `Monitor`: follows a background command's output as it is written. Loses the live view
///   only: `Read` reads the output file, and Claude Code says when the command ends.
/// - `ListAgents` (`ListPeers`, the same tool): lists the agents a session may message. Loses
///   nothing: a helper is continued by the id `Agent` gave.
pub(crate) const NOT_OFFERED: [&str; 17] = [
    "CronCreate",
    "CronDelete",
    "CronList",
    "ScheduleWakeup",
    "RemoteTrigger",
    "EnterWorktree",
    "ExitWorktree",
    "Workflow",
    "REPL",
    "LSP",
    "DesignSync",
    "PushNotification",
    "SendUserMessage",
    "Brief",
    "Monitor",
    "ListAgents",
    "ListPeers",
];

/// The `tools` list for a session: [`CLAUDE_TOOLS`], then [`WINDOWS_TOOLS`] on `windows`, then
/// [`WEB_TOOLS`] with `web_search` on.
pub(crate) fn claude_tools(windows: bool, web_search: bool) -> Vec<&'static str> {
    let mut tools = CLAUDE_TOOLS.to_vec();
    if windows {
        tools.extend(WINDOWS_TOOLS);
    }
    if web_search {
        tools.extend(WEB_TOOLS);
    }
    tools
}

/// Sets `options.tools` to [`claude_tools`] for this system, over any value the hook sent, and
/// appends each of [`NOT_OFFERED`] that `disallowed` lacks.
pub(crate) fn offer(options: &mut Map<String, Value>, disallowed: &mut Vec<Value>, web: bool) {
    let tools = claude_tools(cfg!(windows), web);
    options.insert("tools".to_owned(), Value::from(tools));
    for name in NOT_OFFERED {
        if !disallowed.iter().any(|item| item.as_str() == Some(name)) {
            disallowed.push(Value::from(name));
        }
    }
}
