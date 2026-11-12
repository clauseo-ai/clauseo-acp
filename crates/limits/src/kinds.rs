//! The rules that differ between agent kinds, which the model, the policy and the view read.

use clauseo_protocol::AgentKind;

use crate::view::at_limit_words;

/// What differs between agent kinds. Every per-kind rule of the model reads it, so a new kind
/// is one more arm in [`kind_rules`], which the compiler asks for.
#[derive(Debug, Clone, Copy)]
pub(crate) struct KindRules {
    /// A turn's end that a live signal came before reads when the last read started at least
    /// this long ago, in milliseconds.
    pub(crate) turn_read_after_ms: i64,
    /// A turn's end that no live signal came before reads when the last read started at least
    /// this long ago, in milliseconds.
    pub(crate) turn_read_after_ms_no_live: i64,
    /// A live signal may add a window no read named yet (Claude's 5-hour and weekly windows).
    pub(crate) live_rows: bool,
    /// The at-limit line of an account that pauses at its limit, which names the agent.
    pub(crate) pauses_words: &'static str,
}

/// The rules of `agent`'s kind.
pub(crate) const fn kind_rules(agent: AgentKind) -> KindRules {
    match agent {
        AgentKind::Claude => KindRules {
            turn_read_after_ms: 10 * 60 * 1000,
            turn_read_after_ms_no_live: 2 * 60 * 1000,
            live_rows: true,
            pauses_words: at_limit_words::CLAUDE_PAUSES,
        },
        AgentKind::Codex => KindRules {
            turn_read_after_ms: 60 * 1000,
            turn_read_after_ms_no_live: 60 * 1000,
            live_rows: false,
            pauses_words: at_limit_words::CODEX_PAUSES,
        },
    }
}
