//! `NoticeKind::DownloadOutsideMatter`: a file the browser guard saved in Clauseo's own
//! downloads folder for the matter (`agent::loads::browser::fallback_downloads`), because the
//! matter's drive cannot keep the "downloaded from the internet" mark.
//!
//! The guard tells the agent in a text block of the next `browser` tool result it relays,
//! whichever tool that is (`clauseo_mcp`'s `browser_guard/downloads/saved.rs`):
//! `Saved {name} outside the matter, ... Its path: {absolute path}`. A web page can put any
//! text into a tool result, so the host trusts the block only when it starts with
//! [`SAVED`], holds [`MARKER`], and the text after the last [`MARKER`] is exactly the
//! session's folder joined with one plain name (no separator, `.` or `..`, no control or
//! format character) that `symlink_metadata` finds a regular file, never a link. Each path
//! gives one notice per thread, for the host's run. Read from Claude's `content` and from the
//! MCP result in Codex's `rawOutput` (`{ result, error }`), only when the call completes in
//! the live turn, like `PageVisited`.
//!
//! The check stats one file in the data folder, on the actor: only for a block that already
//! names this session's folder, so a page cannot make it stat anything else.

use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};

use clauseo_acp::ToolCall;
use clauseo_protocol::{
    CompactString, Notice, NoticeKind, NoticeLevel, TextOrBlob, ToolCallContent, ToolCallPatch,
    ToolCallStatus,
};
use serde_json::Value;

/// How the guard's line begins.
const SAVED: &str = "Saved ";

/// What stands right before the path in the guard's line (`saved::FALLBACK_MARKER`).
const MARKER: &str = " Its path: ";

/// The most unfinished `browser` calls a thread keeps; past it they are dropped.
const MAX_CALLS: usize = 64;

/// A thread's unfinished calls of the `browser` server's tools and the files already told.
#[derive(Debug, Default)]
pub(crate) struct Fallbacks {
    /// The agent's ids of the `browser` calls not finished yet.
    calls: BTreeSet<CompactString>,
    /// The paths a notice was given for.
    told: BTreeSet<PathBuf>,
}

impl Fallbacks {
    /// A tool call started: a `browser` one is followed until it ends, or read at once when it
    /// started completed. `folder`: the session's fallback folder as the guard got it.
    pub(crate) fn started(&mut self, call: &ToolCall, folder: Option<&Path>) -> Vec<Notice> {
        if !is_browser_tool(&call.title, call.tool_name.as_deref()) {
            return Vec::new();
        }
        match call.status {
            ToolCallStatus::Completed => self.read(&call.content, call.raw_output.as_ref(), folder),
            ToolCallStatus::Failed | ToolCallStatus::Cancelled => Vec::new(),
            ToolCallStatus::Pending | ToolCallStatus::InProgress | ToolCallStatus::Unknown => {
                if self.calls.len() >= MAX_CALLS {
                    self.calls.clear();
                }
                self.calls.insert(call.acp_id.clone());
                Vec::new()
            }
        }
    }

    /// An update of the call `acp_id`: a `browser` call that completes is read; any final
    /// status forgets it.
    pub(crate) fn updated(
        &mut self,
        acp_id: &str,
        patch: &ToolCallPatch,
        folder: Option<&Path>,
    ) -> Vec<Notice> {
        match patch.status {
            Some(ToolCallStatus::Completed) => {
                if !self.calls.remove(acp_id) {
                    return Vec::new();
                }
                let content = patch.content.as_deref().unwrap_or_default();
                self.read(content, patch.raw_output.as_ref(), folder)
            }
            Some(ToolCallStatus::Failed | ToolCallStatus::Cancelled) => {
                self.calls.remove(acp_id);
                Vec::new()
            }
            Some(
                ToolCallStatus::Pending | ToolCallStatus::InProgress | ToolCallStatus::Unknown,
            )
            | None => Vec::new(),
        }
    }

    /// The notices for the files a completed result names that were not told yet.
    fn read(
        &mut self,
        content: &[ToolCallContent],
        raw_output: Option<&TextOrBlob>,
        folder: Option<&Path>,
    ) -> Vec<Notice> {
        let Some(folder) = folder else {
            return Vec::new();
        };
        let mut notices = Vec::new();
        for text in texts(content, raw_output) {
            let Some((path, name)) = saved_file(&text, folder) else {
                continue;
            };
            if self.told.insert(path) {
                notices.push(notice(name));
            }
        }
        notices
    }
}

/// The notice for a file saved as `name`, with its English fallback text.
fn notice(name: String) -> Notice {
    Notice {
        level: NoticeLevel::Info,
        text: format!(
            "The browser saved {name} in Clauseo's own folder for this matter, not in the \
             matter's folder: the matter's drive can't mark downloads as coming from the internet."
        ),
        kind: Some(NoticeKind::DownloadOutsideMatter { name }),
    }
}

/// Whether a call is one of the `browser` server's tools: Claude's `mcp__browser__<tool>` by
/// its name or title, or Codex's title `mcp.browser.<tool>` (`super::codex_browser_tool`).
fn is_browser_tool(title: &str, tool_name: Option<&str>) -> bool {
    let claude = |name: &str| {
        name.strip_prefix("mcp__")
            .and_then(|rest| rest.strip_prefix(clauseo_profile::servers::BROWSER))
            .and_then(|rest| rest.strip_prefix("__"))
            .is_some_and(|tool| !tool.is_empty())
    };
    super::codex_browser_tool(title).is_some()
        || [Some(title), tool_name].into_iter().flatten().any(claude)
}

/// Every inline text of a result: Claude's text content, then the texts of the MCP result in
/// `rawOutput` (`content`, or Codex's `result.content`).
fn texts(content: &[ToolCallContent], raw_output: Option<&TextOrBlob>) -> Vec<String> {
    let mut texts: Vec<String> = content
        .iter()
        .filter_map(|item| {
            if let ToolCallContent::Text {
                text: TextOrBlob::Inline(text),
            } = item
            {
                Some(text.clone())
            } else {
                None
            }
        })
        .collect();
    if let Some(TextOrBlob::Inline(raw)) = raw_output
        && let Ok(output) = serde_json::from_str::<Value>(raw)
    {
        let result = output.get("result").unwrap_or(&output);
        if let Some(items) = result.get("content").and_then(Value::as_array) {
            texts.extend(
                items
                    .iter()
                    .filter_map(|item| item.get("text")?.as_str().map(str::to_owned)),
            );
        }
    }
    texts
}

/// The file the guard's line `text` names, with its name, when it is one the guard saved in
/// `folder`: see the module's contract.
pub(crate) fn saved_file(text: &str, folder: &Path) -> Option<(PathBuf, String)> {
    if !text.starts_with(SAVED) {
        return None;
    }
    let (_, after) = text.rsplit_once(MARKER)?;
    let path = Path::new(after);
    if !path.is_absolute() {
        return None;
    }
    let mut components = path.components();
    let Some(Component::Normal(name)) = components.next_back() else {
        return None;
    };
    let name = name.to_str()?;
    if !plain_name(name) {
        return None;
    }
    let folder = dunce::simplified(folder);
    // The whole text, not only its parent, so `x/.` or a doubled separator never passes.
    if folder.join(name).as_os_str() != path.as_os_str() {
        return None;
    }
    let metadata = std::fs::symlink_metadata(path).ok()?;
    metadata
        .file_type()
        .is_file()
        .then(|| (path.to_path_buf(), name.to_owned()))
}

/// A name the notice may carry and the reveal recomputes: not empty, no `/` or `\`, not `.`
/// or `..`, and no control or format character (a bidi control could make it read as another
/// name).
fn plain_name(name: &str) -> bool {
    !name.is_empty()
        && Path::new(name).file_name() == Some(OsStr::new(name))
        && !name.contains(['/', '\\'])
        && !name
            .chars()
            .any(|c| c.is_control() || crate::agent::title::is_format(c))
}

#[cfg(test)]
mod tests;
