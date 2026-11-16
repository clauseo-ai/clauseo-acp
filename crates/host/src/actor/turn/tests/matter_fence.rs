//! The fence between matters reaching a live agent (`idle/fence.rs`) with a real `fake_agent`:
//! an invite stops the idle agent and its next launch reads the other matter (its `Read` deny
//! gone, a `Read` allow, its `Edit` deny kept, Codex's note naming it); withdrawing it
//! does the same back; a new matter stops an idle agent; a busy agent is stopped only once its
//! turn ends. Agents' ends are held (`dispatch_holding`, `hold_next`), never raced with
//! `advance` and `drain`.

use std::path::Path;

use clauseo_protocol::{FolderRefId, HostCommand, MatterId, MatterSource, ThreadId, Title};

use super::agents::send_prompt;
use super::idle::{TWO_ANSWERS, idle_stops, live, rig, until_ended, until_idle};
use super::rig::Rig;
use crate::actor::Actor;
use crate::actor::input::Input;
use crate::actor::turn::agents::Stage;
use crate::agent::AgentInput;
use crate::agent::data_fence::{claude_allow_rules, claude_deny_rules, rule_path};
use crate::pipeline::tests::support::TempDir;

/// Registers the folder `name` under `papers` as a matter with command `n`.
pub(super) fn other_matter(rig: &mut Rig, papers: &Path, name: &str, n: u128) -> MatterId {
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

/// The note a Codex agent on the live run is told the matters it may read by
/// (`outside::fence::codex_reads_note`): it names the invited ones.
fn reads_note(rig: &Rig, thread: ThreadId) -> String {
    let run = rig.actor.threads[&thread].turn.run.as_ref().unwrap();
    super::super::outside::fence::codex_reads_note(run)
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
    assert!(!reads_note(&rig, thread).contains("Smith"), "fenced");

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
    // Codex hears the invite in its note, made from this relaunch's run.
    assert!(reads_note(&rig, thread).contains("Smith"), "invited");

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
    assert!(!reads_note(&rig, thread).contains("Smith"), "withdrawn");
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

/// Whether `input` is the agent's or the matter's work on a turn: held to keep a turn running.
fn turn_work(_: &Actor, input: &Input) -> bool {
    matches!(
        input,
        Input::Agent { .. } | Input::Matter { .. } | Input::Turn(_)
    )
}

/// A prompt as command `n`, with the turn's work held.
fn prompt_held(rig: &mut Rig, thread: ThreadId, n: u128) {
    let command = HostCommand::SendPrompt {
        thread,
        content: vec![clauseo_protocol::PromptBlock::Text {
            text: "And the other lease?".to_owned(),
        }],
        delivery: clauseo_protocol::DeliveryRequest::Queue,
    };
    rig.dispatch_holding(n, command, turn_work).unwrap();
}

/// An invite while a turn runs with a prompt queued behind it: the agent is not stopped in the
/// turn, but at its end, before the queued prompt starts; that prompt runs on the relaunched
/// agent, whose rules read the matter. The same holds for a withdrawal (one rule).
#[test]
fn a_busy_agent_with_a_queued_prompt_is_stopped_at_its_turns_end() {
    let mut rig = rig("mfence-busy", TWO_ANSWERS, "exit");
    let papers = TempDir::new("mfence-busy-papers");
    let thread = rig.thread();
    let other = other_matter(&mut rig, papers.path(), "Smith", 5);
    turn(&mut rig, thread, 10);
    until_idle(&mut rig, thread);
    let before = idle_stops(&mut rig, thread);
    prompt_held(&mut rig, thread, 11);
    let allow = HostCommand::AllowMatterRead {
        thread,
        matter: other,
    };
    rig.dispatch_holding(12, allow, turn_work).unwrap();
    prompt_held(&mut rig, thread, 13);
    assert!(live(&rig.actor, thread), "not stopped in its turn");
    assert_eq!(
        rules_for(&rig, thread, other),
        (true, true, false),
        "the old rules still"
    );
    rig.release_held();
    rig.run_until("both turns' ends on the relaunched agent", |actor| {
        let state = &actor.threads[&thread].turn;
        state.current.is_none()
            && state.queue.is_empty()
            && state
                .agent
                .as_deref()
                .is_some_and(|agent| agent.stage == Stage::Attached)
            && actor.work.commits == 0
    });
    assert_eq!(
        idle_stops(&mut rig, thread),
        before + 1,
        "one stop, at the turn's end"
    );
    let log = rig.log(thread);
    let ended = log.iter().filter(|event| {
        matches!(event, clauseo_protocol::DomainEvent::TurnEnded(ended)
            if ended.stop == clauseo_protocol::StopReason::EndTurn)
    });
    assert_eq!(ended.count(), 3);
    assert_eq!(
        rules_for(&rig, thread, other),
        (false, true, true),
        "the relaunch reads it"
    );
    rig.finish();
}

/// A withdrawal followed at once by a prompt: the prompt's command arrives before the next
/// tick, so the idle agent is not stopped there (`idle::pending`); the prompt finds the stale
/// fence when it reaches the agent, stops it, and runs on the relaunch with the matter fenced
/// again.
#[test]
fn a_prompt_sent_right_after_a_withdrawal_runs_on_the_new_rules() {
    let mut rig = rig("mfence-withdraw-send", TWO_ANSWERS, "exit");
    let papers = TempDir::new("mfence-withdraw-send-papers");
    let thread = rig.thread();
    let other = other_matter(&mut rig, papers.path(), "Smith", 5);
    let allow = HostCommand::AllowMatterRead {
        thread,
        matter: other,
    };
    rig.dispatch(6, allow).unwrap();
    turn(&mut rig, thread, 10);
    until_idle(&mut rig, thread);
    assert_eq!(
        rules_for(&rig, thread, other),
        (false, true, true),
        "invited"
    );
    let before = idle_stops(&mut rig, thread);
    let send = |n: u128, command: HostCommand| {
        let (reply, answer) = futures::channel::oneshot::channel();
        let command = clauseo_protocol::CommandEnvelope {
            command_id: super::rig::command_id(n),
            command,
        };
        (
            crate::actor::input::Request::Dispatch { command, reply },
            answer,
        )
    };
    let withdraw = HostCommand::WithdrawMatterRead {
        thread,
        matter: other,
    };
    let prompt = HostCommand::SendPrompt {
        thread,
        content: vec![clauseo_protocol::PromptBlock::Text {
            text: "Now the lease.".to_owned(),
        }],
        delivery: clauseo_protocol::DeliveryRequest::Queue,
    };
    let (first, mut withdrawn) = send(11, withdraw);
    let (second, mut sent) = send(12, prompt);
    rig.actor.on_request(first);
    rig.actor.on_request(second);
    rig.run_until("both answered and the turn ended on a relaunch", |actor| {
        let answered = matches!(withdrawn.try_recv(), Ok(Some(_)) | Err(_))
            && matches!(sent.try_recv(), Ok(Some(_)) | Err(_));
        let state = &actor.threads[&thread].turn;
        answered
            && state.current.is_none()
            && state.queue.is_empty()
            && state
                .agent
                .as_deref()
                .is_some_and(|agent| agent.stage == Stage::Attached)
            && actor.work.commits == 0
    });
    assert_eq!(
        idle_stops(&mut rig, thread),
        before + 1,
        "stopped once, before the prompt"
    );
    assert_eq!(
        rules_for(&rig, thread, other),
        (true, true, false),
        "fenced again"
    );
    rig.finish();
}

/// The limited notice is told once while it lasts; a launch without the limit (any agent but a
/// limited Claude, `profile_loads::claude_fence`) ends it, so the next limited launch tells it
/// again.
#[test]
fn a_launch_without_the_limit_ends_it_so_the_next_tells_again() {
    use clauseo_protocol::{DomainEvent, NoticeKind};

    use super::super::outside::fence::tell_limited;

    let mut rig = Rig::start("turn-shell-limited-again", "");
    let thread = rig.thread();
    let told = |rig: &mut Rig| {
        rig.settle_commits();
        let log = rig.log(thread);
        log.iter()
            .filter(|event| {
                matches!(event, DomainEvent::Notice(notice)
                    if notice.kind == Some(NoticeKind::ShellLimited))
            })
            .count()
    };
    tell_limited(&mut rig.actor, thread, true);
    tell_limited(&mut rig.actor, thread, true);
    assert_eq!(told(&mut rig), 1);
    tell_limited(&mut rig.actor, thread, false);
    tell_limited(&mut rig.actor, thread, true);
    assert_eq!(told(&mut rig), 2);
}
