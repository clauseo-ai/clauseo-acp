//! The subagents Claude may launch through its `Agent` tool, sent as `options.agents`
//! ([`subagents`]): the paper reader and the researcher. Codex gets none: measured, both Codex's
//! own spawning and its batching text were slower and cost more.
//!
//! Each definition is the one measured (stage C's timing harness, arms C2f for the paper reader
//! and RH2 for the researcher), except that neither writes files: each returns its work in its
//! final reply, so neither has a tool that writes, edits or runs a command (review of #1123). The
//! choices the measurements decide are each one constant: [`PAPER_READER_MODEL`] and
//! [`RESEARCHER_ARM`].
//!
//! The model is Claude Code's alias `haiku`, which Claude Code 2.1.293 resolves for the login
//! (its catalog's newest Haiku on Anthropic's own login). Where Haiku is not known to be served
//! (Bedrock, Vertex, Foundry), the host sends no agents and no "Working at scale" part at all
//! (`agent::loads::subagents` in the host). `effort` is read by 2.1.293 for an agent it is given
//! (its agent schema takes it, and a helper's effort is the agent's own) and applies only on a
//! model that takes an effort level; on one that does not, Claude Code drops it without a word.
//!
//! These two are the only helpers a Claude session has: the host turns Claude Code's own agent
//! types and its fork off on every login, and on Anthropic's own login runs every helper on
//! [`PAPER_READER_MODEL`] whatever a definition or the lead's call says
//! (`agent::loads::subagents` in the host).

use serde_json::{Map, Value};

/// The subagent that reads a batch of long matter documents and returns one digest.
pub const PAPER_READER: &str = "paper-reader";

/// The subagent that researches one line on the open web.
pub const RESEARCHER: &str = "researcher";

/// The paper reader's model: `haiku` (arm C2f) or `sonnet` (exp-armb). Measured setting. The
/// host sends no helpers on a login where Haiku is not known to be served.
pub const PAPER_READER_MODEL: &str = "haiku";

/// The paper reader's reasoning effort, as arm C2f ran it. Claude Code applies it only on a model
/// that takes an effort level (see the module doc).
pub const PAPER_READER_EFFORT: &str = "low";

/// The most turns the paper reader takes, as arm C2f ran it.
const PAPER_READER_TURNS: u64 = 25;

/// Which measured researcher Claude gets, or `None` for none, which also leaves out the research
/// part of "Working at scale" (`instructions::SCALE_RESEARCH`, shipped only with
/// `instructions::SHIP_RESEARCH_TEXT`). Measured setting.
pub const RESEARCHER_ARM: Option<ResearcherArm> = Some(ResearcherArm::Rh2);

/// The researchers measured on task T2. Both run on Haiku with the same prompt; RH3 is leaner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResearcherArm {
    /// Medium effort, 30 turns.
    Rh2,
    /// Low effort, 20 turns, and a budget of searches and fetches in its prompt.
    Rh3,
}

impl ResearcherArm {
    /// The researcher's model.
    pub fn model(self) -> &'static str {
        match self {
            Self::Rh2 | Self::Rh3 => "haiku",
        }
    }

    /// The researcher's reasoning effort.
    pub fn effort(self) -> &'static str {
        match self {
            Self::Rh2 => "medium",
            Self::Rh3 => "low",
        }
    }

    /// The most turns the researcher takes.
    pub fn max_turns(self) -> u64 {
        match self {
            Self::Rh2 => 30,
            Self::Rh3 => 20,
        }
    }

    /// What the prompt says between [`RESEARCHER_RULES`] and [`WORK_FAST`]: RH3's budget.
    fn budget(self) -> &'static str {
        match self {
            Self::Rh2 => "",
            Self::Rh3 => {
                " Budget: at most 6 searches and 8 page fetches; stop as soon as the \
                 sub-question is answered from primary sources."
            }
        }
    }
}

/// [`PAPER_READER`]'s description: when the lead launches it.
const PAPER_READER_DESCRIPTION: &str = "Reads a batch of long matter documents from their text \
    copies and returns one short digest. Use 4 to 6 at once, launched in one message, when a task \
    needs more than eight documents read.";

/// [`PAPER_READER`]'s system prompt. Subagents never get `systemPrompt.append`, so it carries its
/// own rules on evidence and untrusted content. It holds `{work_folder}`, which the host fills.
const PAPER_READER_PROMPT: &str = include_str!("../assets/agents/paper-reader.md");

/// [`RESEARCHER`]'s description: what it does, and nothing that urges the lead to launch it;
/// only the research part of "Working at scale" does that (`instructions::SHIP_RESEARCH_TEXT`).
const RESEARCHER_DESCRIPTION: &str = "Researches one legal sub-question or one family of \
    sources (one court, one regulator, one statute) on the open web and returns findings with \
    primary-source citations.";

/// What [`RESEARCHER`]'s prompt holds after the legal part's Evidence and Citations sections
/// ([`legal_evidence_and_citations`]): research rules read only when research is delegated
/// (contrary authority, later history, who in a judgment says what, a provision's provisos, a
/// second route before "nothing found"; legal.md carries the first three in short), then arm
/// RH2's own rules, which say whose names may go into a search.
const RESEARCHER_RULES: &str = include_str!("../assets/agents/researcher.md");

/// The researcher prompt's last sentence.
const WORK_FAST: &str = " Work fast: run your first searches together in one message, then \
    fetch the most promising primary pages together in one message, not one after another.";

/// The paper reader's tools: it reads files, with no shell, no web and no way to write.
pub const PAPER_READER_TOOLS: [&str; 3] = ["Read", "Grep", "Glob"];

/// The researcher's own tools: the web's, with no shell, no file tool and no way to write. It
/// also gets the browser guard's tools ([`researcher_tools`]).
const RESEARCHER_WEB_TOOLS: [&str; 2] = ["WebSearch", "WebFetch"];

/// The tools of the browser guard's MCP server, `clauseo-mcp browser-guard` under the reserved
/// name `crate::servers::BROWSER`: its `ALLOWED_TOOLS` (`crates/mcp/src/browser_guard/filter.rs`,
/// kept equal by a test), each named for Claude as `mcp__browser__<tool>`. A session without the
/// `browser` server has none of them, and Claude Code 2.1.293 leaves a name that matches no tool
/// out of a helper's tools; it refuses a helper only when none of its names match.
pub const BROWSER_GUARD_TOOLS: [&str; 28] = [
    "agent_browser_tools_profiles",
    "agent_browser_open",
    "agent_browser_read",
    "agent_browser_snapshot",
    "agent_browser_back",
    "agent_browser_forward",
    "agent_browser_reload",
    "agent_browser_click",
    "agent_browser_fill",
    "agent_browser_type",
    "agent_browser_press",
    "agent_browser_check",
    "agent_browser_uncheck",
    "agent_browser_select",
    "agent_browser_scroll",
    "agent_browser_wait_ms",
    "agent_browser_wait_for_selector",
    "agent_browser_wait_for_text",
    "agent_browser_wait_for_load",
    "agent_browser_screenshot",
    "agent_browser_get_text",
    "agent_browser_get_url",
    "agent_browser_get_title",
    "agent_browser_tab_new",
    "agent_browser_tab_list",
    "agent_browser_tab_switch",
    "agent_browser_tab_close",
    "agent_browser_close",
];

/// The researcher's tools, by exact name: [`RESEARCHER_WEB_TOOLS`], then each of
/// [`BROWSER_GUARD_TOOLS`] as `mcp__browser__<tool>`. No `Bash` form, in any session.
pub fn researcher_tools() -> Vec<String> {
    let server = crate::servers::BROWSER;
    RESEARCHER_WEB_TOOLS
        .iter()
        .map(|tool| (*tool).to_owned())
        .chain(
            BROWSER_GUARD_TOOLS
                .iter()
                .map(|tool| format!("mcp__{server}__{tool}")),
        )
        .collect()
}

/// The legal part's (`instructions::LEGAL`) `## Evidence` and `## Citations` sections, headings
/// included, verbatim: from `## Evidence` up to the next section after `## Citations`.
pub fn legal_evidence_and_citations() -> &'static str {
    let legal = crate::instructions::LEGAL;
    let start = legal.find("## Evidence").unwrap_or(0);
    let citations = legal[start..]
        .find("## Citations")
        .map_or(start, |at| start + at);
    let end = legal[citations + 2..]
        .find("\n## ")
        .map_or(legal.len(), |at| citations + 2 + at);
    legal[start..end].trim()
}

/// The researcher's prompt for `arm`: the legal part's Evidence and Citations sections, a blank
/// line, then its own rules, the arm's budget and [`WORK_FAST`].
pub fn researcher_prompt(arm: ResearcherArm) -> String {
    format!(
        "{}\n\n{}{}{WORK_FAST}",
        legal_evidence_and_citations(),
        RESEARCHER_RULES.trim(),
        arm.budget()
    )
}

/// The agents Claude may launch, as `options.agents`: [`PAPER_READER`], then [`RESEARCHER`] when
/// [`RESEARCHER_ARM`] names one. The instructions' "Working at scale" part says when to launch
/// them. The paper reader's prompt holds `{work_folder}`, which the host fills; the host also
/// picks each one's model for the login, and leaves the researcher out without web search.
pub fn subagents() -> Map<String, Value> {
    let mut agents = Map::new();
    agents.insert(
        PAPER_READER.to_owned(),
        serde_json::json!({
            "description": PAPER_READER_DESCRIPTION,
            "effort": PAPER_READER_EFFORT,
            "maxTurns": PAPER_READER_TURNS,
            "model": PAPER_READER_MODEL,
            "prompt": PAPER_READER_PROMPT.trim(),
            "tools": PAPER_READER_TOOLS,
        }),
    );
    if let Some(arm) = RESEARCHER_ARM {
        agents.insert(
            RESEARCHER.to_owned(),
            serde_json::json!({
                "description": RESEARCHER_DESCRIPTION,
                "effort": arm.effort(),
                "maxTurns": arm.max_turns(),
                "model": arm.model(),
                "prompt": researcher_prompt(arm),
                "tools": researcher_tools(),
            }),
        );
    }
    agents
}
