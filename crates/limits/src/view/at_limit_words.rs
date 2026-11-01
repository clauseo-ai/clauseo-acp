//! The words of the at-limit line, in one place.

/// A pay-as-you-go account.
pub(crate) const PAY_AS_YOU_GO: &str = "This account pays as it goes, so no plan limits apply.";
/// The workspace has no credits left.
pub(crate) const WORKSPACE_CREDITS: &str =
    "Your workspace is out of credits. Ask your workspace admin to add more.";
/// The workspace reached its spend cap.
pub(crate) const WORKSPACE_SPEND_CAP: &str =
    "Your workspace reached its spend limit. Ask your workspace admin to raise it.";
/// Claude with extra usage turned on.
pub(crate) const EXTRA_USAGE: &str =
    "When you reach a limit, Claude continues on extra usage, billed to your Claude account.";
/// Codex with credits left.
pub(crate) const CREDITS: &str = "When you reach a limit, Codex continues on your plan's credits.";
/// Claude pauses at the limit.
pub(crate) const CLAUDE_PAUSES: &str = "When you reach a limit, Claude pauses until it resets.";
/// Codex pauses at the limit.
pub(crate) const CODEX_PAUSES: &str = "When you reach a limit, Codex pauses until it resets.";
