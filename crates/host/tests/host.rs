//! The host's integration tests, one binary: each module drives real `fake_agent` processes
//! through one part of the host at its public interface (`HostHandle`, the subscriptions, the
//! files in the matter folder). Every test owns a folder under the temp folder and moves time
//! only with `ManualClock`.
//!
//! A test crate root resolves modules beside itself, so each submodule in `host/` names its
//! path. Each carries `#[cfg(test)]` so clippy treats its helpers as test code.
//!
//! `fake_agent` is built only in a workspace run:
//! `cargo nextest run --workspace -E 'package(clauseo-host)'`.

#[cfg(test)]
#[path = "host/observe.rs"]
mod observe;
#[cfg(test)]
#[path = "host/support.rs"]
mod support;
#[cfg(test)]
#[path = "host/write.rs"]
mod write;

#[cfg(test)]
#[path = "host/agent_name.rs"]
mod agent_name;
#[cfg(test)]
#[path = "host/ask_user_question.rs"]
mod ask_user_question;
#[cfg(test)]
#[path = "host/blobs.rs"]
mod blobs;
#[cfg(test)]
#[path = "host/browser.rs"]
mod browser;
#[cfg(test)]
#[path = "host/cancel.rs"]
mod cancel;
#[cfg(test)]
#[path = "host/claude_mode.rs"]
mod claude_mode;
#[cfg(test)]
#[path = "host/codex.rs"]
mod codex;
#[cfg(test)]
#[path = "host/codex_folder.rs"]
mod codex_folder;
#[cfg(test)]
#[path = "host/codex_plans.rs"]
mod codex_plans;
#[cfg(test)]
#[path = "host/commands.rs"]
mod commands;
#[cfg(test)]
#[path = "host/counts.rs"]
mod counts;
#[cfg(test)]
#[path = "host/e2e_scenarios.rs"]
mod e2e_scenarios;
#[cfg(test)]
#[path = "host/forget.rs"]
mod forget;
#[cfg(test)]
#[path = "host/guard_hook.rs"]
mod guard_hook;
#[cfg(test)]
#[path = "host/hindi.rs"]
mod hindi;
#[cfg(test)]
#[path = "host/import.rs"]
mod import;
#[cfg(test)]
#[path = "host/limits.rs"]
mod limits;
#[cfg(test)]
#[path = "host/limits_asks.rs"]
mod limits_asks;
#[cfg(test)]
#[path = "host/limits_host.rs"]
mod limits_host;
#[cfg(test)]
#[path = "host/limits_pending.rs"]
mod limits_pending;
#[cfg(test)]
#[path = "host/limits_stop.rs"]
mod limits_stop;
#[cfg(test)]
#[path = "host/list_agents.rs"]
mod list_agents;
#[cfg(test)]
#[path = "host/lists.rs"]
mod lists;
#[cfg(test)]
#[path = "host/lists_folder.rs"]
mod lists_folder;
#[cfg(test)]
#[path = "host/locate.rs"]
mod locate;
#[cfg(test)]
#[path = "host/matters.rs"]
mod matters;
#[cfg(test)]
#[path = "host/mcp.rs"]
mod mcp;
#[cfg(test)]
#[path = "host/papers.rs"]
mod papers;
#[cfg(test)]
#[path = "host/permissions.rs"]
mod permissions;
#[cfg(test)]
#[path = "host/privacy.rs"]
mod privacy;
#[cfg(test)]
#[path = "host/profile.rs"]
mod profile;
#[cfg(test)]
#[path = "host/profile_rig.rs"]
mod profile_rig;
#[cfg(test)]
#[path = "host/queue.rs"]
mod queue;
#[cfg(test)]
#[path = "host/queue_moves.rs"]
mod queue_moves;
#[cfg(test)]
#[path = "host/queue_restart.rs"]
mod queue_restart;
#[cfg(test)]
#[path = "host/rail.rs"]
mod rail;
#[cfg(test)]
#[path = "host/refusals.rs"]
mod refusals;
#[cfg(test)]
#[path = "host/relaunch.rs"]
mod relaunch;
#[cfg(test)]
#[path = "host/relocate.rs"]
mod relocate;
#[cfg(test)]
#[path = "host/restart.rs"]
mod restart;
#[cfg(test)]
#[path = "host/search.rs"]
mod search;
#[cfg(test)]
#[path = "host/search_review.rs"]
mod search_review;
#[cfg(test)]
#[path = "host/settings.rs"]
mod settings;
#[cfg(test)]
#[path = "host/steer.rs"]
mod steer;
#[cfg(test)]
#[path = "host/subscribe.rs"]
mod subscribe;
#[cfg(all(test, unix))]
#[path = "host/symlink.rs"]
mod symlink;
#[cfg(test)]
#[path = "host/tool_switches.rs"]
mod tool_switches;
#[cfg(test)]
#[path = "host/tools.rs"]
mod tools;

#[cfg(test)]
#[path = "host/titles.rs"]
mod titles;
#[cfg(test)]
#[path = "host/turn.rs"]
mod turn;
#[cfg(test)]
#[path = "host/versions.rs"]
mod versions;
#[cfg(test)]
#[path = "host/word_open.rs"]
mod word_open;

#[cfg(test)]
#[path = "host/word_round_trip.rs"]
mod word_round_trip;
#[cfg(test)]
#[path = "host/writer.rs"]
mod writer;
