//! The launch's say over Claude's subagents (`clauseo_profile::subagents`, sent as
//! `claudeCode.options.agents`) and over the "Working at scale" part that sends work to them
//! (`clauseo_profile::instructions::claude_scale`, the end of `systemPrompt.append`).
//!
//! **A cloud login gets neither.** The profile names Claude Code's alias `haiku`, as measured.
//! On Anthropic's own login Claude Code 2.1.293 resolves it to its catalog's newest Haiku, which
//! every account there is served. On Bedrock, Vertex or Foundry it resolves to that cloud's Haiku
//! id, which an account has only if its firm enabled that model in that region, and Clauseo
//! cannot see that before `session/new` answers. Running the helpers on the conversation's model
//! instead would send 4 to 6 readers on the lawyer's own model (Opus, say) without telling them.
//! So such a session gets no `agents` and no "Working at scale" part: one agent, as before the
//! helpers, and one log line says why. A firm's `availableModels` list without Haiku needs
//! nothing here: Claude Code then runs the helper on the newest allowed model of its family, or
//! the conversation's, and logs a warning.
//!
//! **The researcher** has only the web's tools and the browser guard's
//! (`clauseo_profile::subagents::researcher_tools`). With web search off the web's are denied,
//! and in a session without the browser none of the guard's exist, so Claude Code would refuse to
//! start it ("would be spawned with zero tools"); it is left out instead. The research bullets
//! (`SCALE_RESEARCH`, shipped only with `SHIP_RESEARCH_TEXT`) go in only beside a researcher
//! that stayed, so no session is told about a helper it lacks.

use serde_json::{Map, Value};

use super::profile::Login;
use clauseo_profile::instructions::{SCALE_RESEARCH, SHIP_RESEARCH_TEXT, claude_scale};
use clauseo_profile::subagents::RESEARCHER;

/// Sets `options.agents` and the end of `meta`'s `systemPrompt.append` for `login` and
/// `web_search` (see the module doc), before the host fills the append's tokens.
pub(crate) fn subagent_meta(
    meta: &mut Map<String, Value>,
    options: &mut Map<String, Value>,
    login: Login,
    web_search: bool,
) {
    helpers(meta, options, login, web_search, SHIP_RESEARCH_TEXT);
}

/// [`subagent_meta`] with `ship_research` in place of `SHIP_RESEARCH_TEXT`.
pub(crate) fn helpers(
    meta: &mut Map<String, Value>,
    options: &mut Map<String, Value>,
    login: Login,
    web_search: bool,
    ship_research: bool,
) {
    // The profile's append ends with a blank line and the part, without the research bullets.
    let scale = claude_scale(false).map(|scale| format!("\n\n{scale}"));
    let append = meta
        .get_mut("systemPrompt")
        .and_then(Value::as_object_mut)
        .and_then(|prompt| prompt.get_mut("append"))
        .and_then(|text| {
            if let Value::String(text) = text {
                Some(text)
            } else {
                None
            }
        })
        .filter(|text| {
            scale
                .as_ref()
                .is_some_and(|scale| text.ends_with(scale.as_str()))
        });
    if login != Login::FirstParty {
        let had_agents = options.remove("agents").is_some();
        let had_scale = match (append, &scale) {
            (Some(text), Some(scale)) => {
                text.truncate(text.len() - scale.len());
                true
            }
            _ => false,
        };
        if had_agents || had_scale {
            tracing::info!(
                ?login,
                "Claude gets no helpers and no \"Working at scale\" part: Haiku is not known to \
                 be served on this login, and they would run on the lawyer's own model"
            );
        }
        return;
    }
    if let Some(Value::Object(agents)) = options.get_mut("agents") {
        if !web_search {
            agents.remove(RESEARCHER);
        }
        if agents.is_empty() {
            options.remove("agents");
        }
    }
    let researcher = options
        .get("agents")
        .and_then(|agents| agents.get(RESEARCHER))
        .is_some();
    if let (true, true, Some(text)) = (ship_research, researcher, append) {
        text.push('\n');
        text.push_str(SCALE_RESEARCH.trim());
    }
}
