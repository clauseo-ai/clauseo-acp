//! The tool-call source (all OSes; the only one on Windows today): a tool call that ends
//! `failed` with an access-denied message, on a path outside the jail, is read as a refusal.
//! Its path and its message are the agent's own words, so the offer rules' card is cut down by
//! the ruling (`offer/ruling.rs`): another matter only when an OS report confirms it, otherwise
//! one file the launch would deny. A later PR swaps [`says_access_denied`] for the sandbox's
//! `Launchers::would_deny`.

use clauseo_protocol::{
    ContentBlock, TextOrBlob, ToolCallContent, ToolCallPatch, ToolCallStatus, ToolLocation,
};

#[cfg(test)]
mod tests;

/// What one tool call or update says, as the denial watch reads it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Seen<'a> {
    pub(crate) status: Option<ToolCallStatus>,
    pub(crate) locations: &'a [ToolLocation],
    pub(crate) raw_input: Option<&'a TextOrBlob>,
    pub(crate) raw_output: Option<&'a TextOrBlob>,
    pub(crate) content: &'a [ToolCallContent],
}

impl<'a> Seen<'a> {
    /// A tool call as it started.
    pub(crate) fn call(call: &'a clauseo_acp::ToolCall) -> Self {
        Self {
            status: Some(call.status),
            locations: &call.locations,
            raw_input: call.raw_input.as_ref(),
            raw_output: call.raw_output.as_ref(),
            content: &call.content,
        }
    }

    /// An update.
    pub(crate) fn patch(patch: &'a ToolCallPatch) -> Self {
        Self {
            status: patch.status,
            locations: patch.locations.as_deref().unwrap_or_default(),
            raw_input: patch.raw_input.as_ref(),
            raw_output: patch.raw_output.as_ref(),
            content: patch.content.as_deref().unwrap_or_default(),
        }
    }

    /// The paths it names: each location (a matter path relative, an outside one as the agent
    /// gave it), then `rawInput`'s `file_path` or `path`.
    pub(crate) fn paths(&self) -> Vec<String> {
        let mut paths: Vec<String> = self
            .locations
            .iter()
            .filter_map(|location| match location {
                ToolLocation::Inside { path, .. } => Some(path.as_str().to_owned()),
                ToolLocation::Outside { display } => Some(display.clone()),
                ToolLocation::Unknown { .. } => None,
            })
            .collect();
        if let Some(TextOrBlob::Inline(text)) = self.raw_input
            && let Ok(serde_json::Value::Object(input)) = serde_json::from_str(text)
        {
            let named = ["file_path", "path"]
                .iter()
                .find_map(|key| input.get(*key)?.as_str());
            paths.extend(named.map(str::to_owned));
        }
        paths
    }

    /// Whether it ended `failed` with an access-denied message in its content or its output.
    pub(crate) fn failed_access_denied(&self) -> bool {
        if self.status != Some(ToolCallStatus::Failed) {
            return false;
        }
        let inline = |text: &'a TextOrBlob| match text {
            TextOrBlob::Inline(text) => Some(text.as_str()),
            TextOrBlob::Blob(_) => None,
        };
        let content = self.content.iter().filter_map(|item| match item {
            ToolCallContent::Text { text } => inline(text),
            ToolCallContent::Content {
                content: ContentBlock::Text { text },
            } => Some(text.as_str()),
            ToolCallContent::Content { .. }
            | ToolCallContent::Diff { .. }
            | ToolCallContent::Unknown { .. } => None,
        });
        content
            .chain(self.raw_output.and_then(inline))
            .any(says_access_denied)
    }
}

/// Whether `text` says access was denied: Windows' "Access is denied" or
/// `ERROR_ACCESS_DENIED`, POSIX's "Permission denied" or "Operation not permitted", or the codes
/// `EACCES` and `EPERM` as whole words. Without case.
pub(crate) fn says_access_denied(text: &str) -> bool {
    const PHRASES: [&str; 4] = [
        "access is denied",
        "access denied",
        "permission denied",
        "operation not permitted",
    ];
    const CODES: [&str; 3] = ["error_access_denied", "eacces", "eperm"];
    let lower = text.to_ascii_lowercase();
    PHRASES.iter().any(|phrase| lower.contains(phrase))
        || CODES.iter().any(|code| has_word(&lower, code))
}

/// Whether `word` appears in `text` with no letter, digit or `_` on either side.
fn has_word(text: &str, word: &str) -> bool {
    let is_word = |c: char| c.is_ascii_alphanumeric() || c == '_';
    text.match_indices(word).any(|(at, _)| {
        let before = text.get(..at).and_then(|head| head.chars().next_back());
        let after = text
            .get(at.saturating_add(word.len())..)
            .and_then(|tail| tail.chars().next());
        !before.is_some_and(is_word) && !after.is_some_and(is_word)
    })
}
