//! A launch's wait for the profile's skills (`ProfileHook::launch_wait`): read on the actor with
//! the skills writes, waited for on the launch's prep task, never the actor. While a background
//! step that may change the writes is in flight (desktop's stetdoc try after an install or an
//! update), the prep waits until the step ends, then writes what the hook has then; past
//! [`LAUNCH_WAIT_LIMIT`] from the launch, it writes what it read at the launch. The turn stays
//! "starting" meanwhile (`ThreadStatus::Starting`), as for any launch's prep. A launch with
//! nothing in flight does not wait, and an agent without a kind gets no skills and no wait.

use std::future::Future;
use std::pin::Pin;

use clauseo_profile::SkillWrite;
use clauseo_protocol::{AgentId, AgentKind};
use futures::future::{Either, select};

use crate::actor::Actor;
use crate::config::{LAUNCH_WAIT_LIMIT, LaunchWait};
use crate::log_file::LIFECYCLE;

/// A launch's skills: the writes read at the launch, and the wait for the step in flight.
#[derive(Default)]
pub(super) struct SkillsPrep {
    writes: Vec<SkillWrite>,
    wait: Option<Wait>,
}

/// The hook's wait and the launch's limit, armed on the host clock at the launch.
struct Wait {
    step: LaunchWait,
    limit: Pin<Box<dyn Future<Output = ()> + Send>>,
}

/// The skills of `agent`'s launch, read on the actor: none for an agent without a kind.
pub(super) fn skills_prep(actor: &Actor, agent: &AgentId) -> SkillsPrep {
    if AgentKind::of(agent).is_none() {
        return SkillsPrep::default();
    }
    let wait = actor.profile.launch_wait(agent).map(|step| {
        tracing::info!(
            target: LIFECYCLE,
            limit_secs = LAUNCH_WAIT_LIMIT.as_secs(),
            "the launch waits for the profile's skills (stetdoc is being tried)"
        );
        Wait {
            step,
            limit: actor.clock.after(LAUNCH_WAIT_LIMIT),
        }
    });
    SkillsPrep {
        writes: actor.profile.launch_writes(agent),
        wait,
    }
}

/// `keys_wait` (`sign_in::keys_wait`) and the skills' wait together, on the prep task: the
/// writes to run once both have ended.
pub(super) async fn after_keys(
    keys_wait: Option<impl Future<Output = ()>>,
    skills: SkillsPrep,
) -> Vec<SkillWrite> {
    let keys = async {
        if let Some(keys_wait) = keys_wait {
            keys_wait.await;
        }
    };
    let ((), writes) = futures::join!(keys, skills.settle());
    writes
}

impl SkillsPrep {
    /// The writes once the step ends, or those read at the launch when there is no step or the
    /// limit passes first.
    async fn settle(self) -> Vec<SkillWrite> {
        let Some(Wait { step, limit }) = self.wait else {
            return self.writes;
        };
        match select(step, limit).await {
            Either::Left((writes, _)) => {
                tracing::info!(
                    target: LIFECYCLE,
                    "the profile's skills are ready; the launch goes on"
                );
                writes
            }
            Either::Right(((), _)) => {
                tracing::warn!(
                    target: LIFECYCLE,
                    limit_secs = LAUNCH_WAIT_LIMIT.as_secs(),
                    "the profile's skills were not ready in time; the launch goes on without them"
                );
                self.writes
            }
        }
    }
}
