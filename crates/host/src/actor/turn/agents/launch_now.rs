//! The launch on the actor: the agent's process, its `initialize`, and the agent as the actor
//! holds it.

use std::collections::BTreeMap;

use clauseo_process::TreeWatch;
use clauseo_protocol::ThreadId;

use super::super::super::Actor;
use super::super::launch_checks::Prepared;
use super::super::{AgentReply, session, state};
use super::{Agent, Stage, StopCause, Waiting, agent, call, fail_all, launch, start_timer};
use crate::log_file::LIFECYCLE;
use crate::rate::RateCap;

/// The launch on the actor, with what its checks found (`prepared`), and `initialize`. When
/// the launch failed after the agent's process was started, answers its tree, for the run
/// folder's removal to wait for.
pub(in super::super) fn launch_now(
    actor: &mut Actor,
    thread: ThreadId,
    waiting: Vec<Waiting>,
    prepared: Prepared,
) -> Option<TreeWatch> {
    // Only a Claude launch has a folder answer without a Codex preparation.
    let unprotected = prepared.folder.is_some_and(agent::forced_ask::applies);
    let forced_ask = prepared.codex.is_none() && unprotected;
    let prepared_folder = prepared.folder;
    let profile = prepared.profile;
    let shell_start = if prepared.settings.shell_carries {
        crate::sent::ShellStart::Unknown
    } else {
        crate::sent::ShellStart::Matter
    };
    if forced_ask {
        let matter = prepared.run.key.matter;
        tracing::info!(%matter, forced_ask, "Claude runs in forced ask: the matter folder is unprotected");
    }
    let mut tree = None;
    let (link, folder, codex, plan_notes, matter_spellings, claude_home, fence_vars, probe) =
        match launch(actor, thread, prepared, &mut tree) {
            Ok(launched) => launched,
            Err(error) => {
                fail_all(actor, thread, waiting, &error);
                return tree;
            }
        };
    let id = start_timer::launched(actor, thread);
    let client = link.client.clone();
    call(actor, thread, id, async move {
        AgentReply::Initialized(client.initialize().await)
    });
    let agent_id = actor
        .check_thread(thread)
        .map(|entry| entry.row.agent.clone())
        .ok();
    tracing::info!(
        target: LIFECYCLE,
        %thread,
        agent = agent_id.as_ref().map(clauseo_protocol::AgentId::as_str),
        pid = link.pid(),
        "agent launched"
    );
    let agent = Agent {
        id,
        link,
        stage: Stage::Initializing,
        info: None,
        waiting,
        after_stop: Vec::new(),
        folder,
        shell_start,
        stop_cause: StopCause::Asked,
        session: None,
        rate: RateCap::new(),
        rate_armed: false,
        permissions: BTreeMap::new(),
        elicitations: BTreeMap::new(),
        url_elicitations: BTreeMap::new(),
        messages: BTreeMap::new(),
        run: None,
        compaction: BTreeMap::new(),
        replay: None,
        replayed: Vec::new(),
        preparing: false,
        codex,
        probe,
        mcp_commands: Vec::new(),
        forced_ask,
        folder_check: prepared_folder,
        auto: session::Auto::default(),
        claude_home,
        mcp_sent: (!unprotected).then(Vec::new),
        plan_notes,
        fence_vars,
        denied_data_folder: 0,
        start_failure: None,
        matter_spellings,
        idle: super::super::idle::Mark::on_profile(profile),
        guard_blocks: super::super::updates::guard_blocks::Reads::default(),
    };
    if let Some(state) = state(actor, thread) {
        state.agent = Some(Box::new(agent));
    }
    super::super::idle::launched(actor, thread);
    None
}
