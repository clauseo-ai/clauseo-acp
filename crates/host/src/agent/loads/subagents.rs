//! The launch's say over Claude's subagents (`clauseo_profile::subagents`, sent as
//! `claudeCode.options.agents`): which model each runs on, and whether the researcher goes at all.
//!
//! **The model.** The profile names Claude Code's alias `haiku`, as measured. Claude Code 2.1.293
//! resolves it for the login: on Anthropic's own login to its catalog's newest Haiku, which every
//! account there is served; on Bedrock, Vertex or Foundry to that cloud's Haiku id, which an
//! account has only if its firm enabled that model in that region. Clauseo cannot see that before
//! `session/new` answers (the model list comes back in the answer), and a helper whose model the
//! cloud refuses fails mid-answer, in front of the lawyer. So on a cloud login each helper gets
//! `inherit`, the conversation's own model, which the session already runs on, and the host logs
//! the swap. A firm's `availableModels` list without Haiku needs nothing here: Claude Code then
//! runs the helper on the newest allowed model of its family, or the conversation's, and logs a
//! warning (its `Subagent model "…" is not in the availableModels allowlist`).
//!
//! **The researcher** has only the web's tools and the browser guard's
//! (`clauseo_profile::subagents::researcher_tools`). With web search off the web's are denied,
//! and in a session without the browser none of the guard's exist, so Claude Code would refuse to
//! start it ("would be spawned with zero tools"); it is left out instead.

use serde_json::{Map, Value};

use super::profile::Login;
use clauseo_profile::subagents::RESEARCHER;

/// What a helper's `model` becomes where Haiku is not known to be served: Claude Code's word for
/// the conversation's model.
pub(crate) const INHERIT: &str = "inherit";

/// Sets each agent's `model` under `options.agents` for `login` (see the module doc) and drops
/// the researcher when `web_search` is off. Without `agents`, nothing changes; an `agents` left
/// empty is removed.
pub(crate) fn subagent_options(options: &mut Map<String, Value>, login: Login, web_search: bool) {
    let Some(Value::Object(agents)) = options.get_mut("agents") else {
        return;
    };
    if !web_search {
        agents.remove(RESEARCHER);
    }
    if login != Login::FirstParty {
        let mut swapped = Vec::new();
        for (name, agent) in agents.iter_mut() {
            let Some(model) = agent.get_mut("model") else {
                continue;
            };
            if model.as_str() != Some(INHERIT) {
                *model = Value::String(INHERIT.to_owned());
                swapped.push(name.clone());
            }
        }
        if !swapped.is_empty() {
            tracing::info!(
                ?login,
                agents = ?swapped,
                "Claude's helpers run on the conversation's model: Haiku is not known to be \
                 served on this login"
            );
        }
    }
    if agents.is_empty() {
        options.remove("agents");
    }
}
