//! The fence between matters reaching a live agent (`idle/fence.rs`) with a real `fake_agent`:
//! an invite stops the idle agent and its next launch reads the other matter (its `Read` deny
//! gone, a `Read` allow, its `Edit` deny kept); withdrawing it does the same back; a new matter
//! stops an idle agent; a busy agent is stopped only once its turn ends. Agents' ends are held
//! (`dispatch_holding`, `hold_next`), never raced with `advance` and `drain`.

use std::path::Path;

use clauseo_protocol::{FolderRefId, HostCommand, MatterId, MatterSource, ThreadId, Title};

use super::agents::send_prompt;
use super::idle::{TWO_ANSWERS, idle_stops, live, rig, until_ended, until_idle};
use super::rig::Rig;
use crate::actor::Actor;
use crate::actor::input::Input;
use crate::agent::AgentInput;
use crate::agent::data_fence::{claude_allow_rules, claude_deny_rules, rule_path};
use crate::pipeline::tests::support::TempDir;

/// Registers the folder `name` under `papers` as a matter with command `n`.
fn other_matter(rig: &mut Rig, papers: &Path, name: &str, n: u128) -> MatterId {
    let folder = papers.join(name);
    crate::write::create_dir_all(&folder).unwrap();
    let now = rig.actor.now().wall;
    let id: FolderRefId = rig.actor.ids.mint(now).unwrap();
    rig.actor.folders.insert(id, folder);
    let before: Vec<MatterId> = rig.actor.matters.keys().copied().collect();
    let command = HostCommand::CreateMatter {
        name: Title::new(name).unwrap(),
        source: MatterSource::ExistingFolder { folder: id },
        client: None,
        next_date: None,
    };
    rig.dispatch(n, command).unwrap();
    let matter = rig.actor.matters.keys().find(|id| !before.contains(id));
    *matter.unwrap()
}

/// Whether `input` is an agent's end.
fn agent_end(_: &Actor, input: &Input) -> bool {
    matches!(
        input,
        Input::Agent {
            input: AgentInput::Ended(_),
            ..
        }
    )
}

/// The live run's Claude rules naming `matter`'s folder: (Read denied, Edit denied, Read allowed).
fn rules_for(rig: &Rig, thread: ThreadId, matter: MatterId) -> (bool, bool, bool) {
    let run = rig.actor.threads[&thread].turn.run.as_ref().unwrap();
    let other = run
        .data_deny
        .matters
        .others
        .iter()
        .find(|other| other.id == matter)
        .unwrap();
    let folder = other.spellings.first().unwrap().to_str().unwrap();
    let base = rule_path(folder, cfg!(windows));
    let deny = claude_deny_rules(&run.data_deny);
    let allow = claude_allow_rules(&run.data_deny);
    let has = |rules: &[String], rule: String| rules.contains(&rule);
    (
        has(&deny, format!("Read({base}/**)")),
        has(&deny, format!("Edit({base}/**)")),
        has(&allow, format!("Read({base}/**)")),
    )
}

/// Dispatches `command` as `n` with the agent's end held: the idle agent is stopping, then
/// ends with one more `IdleStopped`.
fn stops_idle(rig: &mut Rig, thread: ThreadId, n: u128, command: HostCommand) {
    let before = idle_stops(rig, thread);
    rig.dispatch_holding(n, command, agent_end).unwrap();
    rig.run_until("the stop", |actor| {
        !live(actor, thread) && actor.work.commits == 0
    });
    rig.release_held();
    until_ended(rig, thread);
    assert_eq!(idle_stops(rig, thread), before + 1);
}

/// Sends prompt `n` and runs its turn to the end on a fresh launch.
fn turn(rig: &mut Rig, thread: ThreadId, n: u128) {
    send_prompt(rig, n, thread);
    rig.run_until("the turn's end", |actor| {
        let state = &actor.threads[&thread].turn;
        state.current.is_none() && state.agent.is_some() && actor.work.commits == 0
    });
}

#[test]
fn an_invite_and_its_withdrawal_each_stop_the_idle_agent_and_change_the_relaunchs_rules() {
    let mut rig = rig("mfence-invite", TWO_ANSWERS, "exit");
    let papers = TempDir::new("mfence-invite-papers");
    let thread = rig.thread();
    let other = other_matter(&mut rig, papers.path(), "Smith", 5);
    turn(&mut rig, thread, 10);
    until_idle(&mut rig, thread);
    assert_eq!(
        rules_for(&rig, thread, other),
        (true, true, false),
        "fenced"
    );

    stops_idle(
        &mut rig,
        thread,
        11,
        HostCommand::AllowMatterRead {
            thread,
            matter: other,
        },
    );
    turn(&mut rig, thread, 12);
    assert_eq!(
        rules_for(&rig, thread, other),
        (false, true, true),
        "invited"
    );

    until_idle(&mut rig, thread);
    let withdraw = HostCommand::WithdrawMatterRead {
        thread,
        matter: other,
    };
    stops_idle(&mut rig, thread, 13, withdraw);
    turn(&mut rig, thread, 14);
    assert_eq!(
        rules_for(&rig, thread, other),
        (true, true, false),
        "fenced again"
    );
    rig.finish();
}

#[test]
fn a_new_matter_stops_the_idle_agent_and_its_relaunch_fences_it() {
    let mut rig = rig("mfence-new-matter", TWO_ANSWERS, "exit");
    let papers = TempDir::new("mfence-new-matter-papers");
    let thread = rig.thread();
    turn(&mut rig, thread, 10);
    until_idle(&mut rig, thread);
    let before = idle_stops(&mut rig, thread);
    let folder = papers.path().join("Jones");
    crate::write::create_dir_all(&folder).unwrap();
    let now = rig.actor.now().wall;
    let id: FolderRefId = rig.actor.ids.mint(now).unwrap();
    rig.actor.folders.insert(id, folder);
    let create = HostCommand::CreateMatter {
        name: Title::new("Jones").unwrap(),
        source: MatterSource::ExistingFolder { folder: id },
        client: None,
        next_date: None,
    };
    rig.dispatch_holding(11, create, agent_end).unwrap();
    rig.run_until("the stop", |actor| {
        !live(actor, thread) && actor.work.commits == 0
    });
    rig.release_held();
    until_ended(&mut rig, thread);
    assert_eq!(idle_stops(&mut rig, thread), before + 1);
    turn(&mut rig, thread, 12);
    let jones = *rig
        .actor
        .matters
        .keys()
        .find(|id| **id != rig.matter())
        .unwrap();
    assert_eq!(rules_for(&rig, thread, jones), (true, true, false));
    rig.finish();
}
