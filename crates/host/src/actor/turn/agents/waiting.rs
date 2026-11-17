//! The work that waits for an agent's session, run once it is attached or failed when it can't
//! be: a turn at step 2, `SetConfigOption` and `SetMode`. Moved out of `agents.rs` as it is.

use clauseo_protocol::{ConfigValue, ErrorCode, ThreadId};

use super::super::super::Actor;
use super::super::{AgentReply, lost, state, steps};
use super::{Waiting, call};
use crate::agent;
use crate::error::Error;

pub(in crate::actor::turn) fn run_waiting(actor: &mut Actor, thread: ThreadId, waiting: Waiting) {
    let Some(agent) = state(actor, thread).and_then(|state| state.agent.as_deref()) else {
        fail_waiting(actor, thread, &waiting, &lost("the agent"));
        return;
    };
    let (id, client, forced) = (agent.id, agent.link.client.clone(), agent.forced_ask);
    let Some(session) = agent.session.clone() else {
        fail_waiting(actor, thread, &waiting, &lost("the agent's session"));
        return;
    };
    match waiting {
        Waiting::Turn(turn) => steps::on_launched(actor, thread, turn),
        Waiting::Config(ctx, option, value)
            if forced && !agent::forced_ask::config_allowed(option.as_str(), select(&value)) =>
        {
            let error = Error::refused(
                ErrorCode::CommandInvalid,
                format!(
                    "config option {option} sets the mode, and this matter's folder is \
                     unprotected: only \"default\" and \"plan\" may be set"
                ),
            );
            actor.finish_command(ctx.id, Err(error));
        }
        Waiting::Config(ctx, option, value) => {
            super::super::idle::call_started(actor, thread);
            call(actor, thread, id, async move {
                let result = client.set_config_option(&session, option.clone(), value.clone());
                let (pick, result) = ((option, value), result.await);
                AgentReply::Config { ctx, pick, result }
            });
        }
        Waiting::Mode(ctx, mode) if forced && !agent::forced_ask::mode_allowed(mode.as_str()) => {
            let error = Error::refused(
                ErrorCode::CommandInvalid,
                format!(
                    "mode {mode} would approve changes without asking, and this matter's folder \
                     is unprotected: only \"default\" and \"plan\" may be set"
                ),
            );
            actor.finish_command(ctx.id, Err(error));
        }
        Waiting::Mode(ctx, mode) => {
            super::super::idle::call_started(actor, thread);
            call(actor, thread, id, async move {
                let result = client.set_mode(&session, mode.clone()).await;
                AgentReply::Mode { ctx, mode, result }
            });
        }
    }
}

/// The choice id `value` names, for a select option; `None` for a boolean.
fn select(value: &ConfigValue) -> Option<&str> {
    match value {
        ConfigValue::Select(choice) => Some(choice.as_str()),
        ConfigValue::Boolean(_) => None,
    }
}

/// Fails each of `waiting` with `error`.
pub(in crate::actor::turn) fn fail_all(
    actor: &mut Actor,
    thread: ThreadId,
    waiting: Vec<Waiting>,
    error: &Error,
) {
    for item in waiting {
        fail_waiting(actor, thread, &item, error);
    }
}

pub(super) fn fail_waiting(actor: &mut Actor, thread: ThreadId, waiting: &Waiting, error: &Error) {
    match waiting {
        Waiting::Turn(turn) => steps::on_launch_failed(actor, thread, *turn, error),
        Waiting::Config(ctx, ..) | Waiting::Mode(ctx, ..) => {
            let error = Error::refused(error.code(), error.to_string());
            actor.finish_command(ctx.id, Err(error));
        }
    }
}
