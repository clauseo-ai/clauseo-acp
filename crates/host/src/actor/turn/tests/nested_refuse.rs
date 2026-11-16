//! A matter on the list inside the conversation's own (`agent::matter_fence::held`), with a
//! real `fake_agent`: the launch is refused naming the inner matter, so is a resume after a
//! restart and the next turn of an open conversation whose folders moved (a link repointed), and
//! a case-variant spelling is caught where names fold; the inner matter's conversation runs, and
//! a matter whose folder merely starts with the same letters refuses nothing. The layout is set
//! on the host's rows, as folders moved outside Clauseo leave it: no command makes it.

use std::path::{Path, PathBuf};

use clauseo_protocol::{
    AgentId, CommandOutcome, DomainEvent, ErrorCode, HostCommand, MatterId, NoticeKind, ThreadId,
};

use super::agents::send_prompt;
use super::idle::{TWO_ANSWERS, live, rig, until_idle};
use super::matter_fence::other_matter;
use super::rig::Rig;
use crate::pipeline::tests::support::TempDir;

/// The Scratch matter's folder.
fn scratch(rig: &Rig) -> PathBuf {
    PathBuf::from(&rig.actor.matters[&rig.matter()].row.folder_path)
}

/// Points `matter`'s row at `folder`, made, as a move outside Clauseo leaves it.
fn move_to(rig: &mut Rig, matter: MatterId, folder: &Path) {
    crate::write::create_dir_all(folder).unwrap();
    let row = &mut rig.actor.matters.get_mut(&matter).unwrap().row;
    row.folder_path = folder.to_string_lossy().into_owned();
}

/// Sends prompt `n` and runs until its turn has closed and nothing is in flight.
fn turn(rig: &mut Rig, thread: ThreadId, n: u128) {
    send_prompt(rig, n, thread);
    rig.run_until("the turn's end", |actor| {
        let state = &actor.threads[&thread].turn;
        state.current.is_none() && state.preparing.is_none() && actor.work.commits == 0
    });
}

/// The codes of `thread`'s failed turns, and the inner matters its notices named, in order.
fn told(rig: &mut Rig, thread: ThreadId) -> (Vec<ErrorCode>, Vec<(MatterId, String)>) {
    let log = rig.log(thread);
    let failed = log.iter().filter_map(|event| match event {
        DomainEvent::TurnFailed(failed) => Some(failed.code),
        _ => None,
    });
    let inside = log.iter().filter_map(|event| match event {
        DomainEvent::Notice(notice) => match &notice.kind {
            Some(NoticeKind::MatterInside { matter, name }) => Some((*matter, name.clone())),
            _ => None,
        },
        _ => None,
    });
    (failed.collect(), inside.collect())
}

/// Whether the notice comes right before the failure in `thread`'s log.
fn notice_then_failure(rig: &mut Rig, thread: ThreadId) -> bool {
    let log = rig.log(thread);
    log.windows(2).any(|pair| {
        let notice = matches!(&pair[0], DomainEvent::Notice(notice)
            if matches!(notice.kind, Some(NoticeKind::MatterInside { .. })));
        let failed = matches!(&pair[1], DomainEvent::TurnFailed(failed)
            if failed.code == ErrorCode::MatterHoldsOtherMatter);
        notice && failed
    })
}

const HELD: ErrorCode = ErrorCode::MatterHoldsOtherMatter;

#[test]
fn a_launch_in_a_matter_holding_a_matter_on_the_list_is_refused_naming_it() {
    let mut rig = rig("held-launch", TWO_ANSWERS, "exit");
    let papers = TempDir::new("held-launch-papers");
    let thread = rig.thread();
    let sub = other_matter(&mut rig, papers.path(), "Sub", 5);
    let inner = scratch(&rig).join("W").join("Sub");
    move_to(&mut rig, sub, &inner);
    turn(&mut rig, thread, 10);
    assert_eq!(
        told(&mut rig, thread),
        (vec![HELD], vec![(sub, "Sub".into())])
    );
    assert!(notice_then_failure(&mut rig, thread));
    assert!(
        rig.actor.threads[&thread].turn.agent.is_none(),
        "no agent started"
    );
    rig.finish();
}

#[test]
fn a_resume_after_a_restart_is_refused() {
    let mut rig = rig("held-resume", TWO_ANSWERS, "exit");
    let papers = TempDir::new("held-resume-papers");
    let thread = rig.thread();
    let sub = other_matter(&mut rig, papers.path(), "Sub", 5);
    turn(&mut rig, thread, 10);
    assert_eq!(told(&mut rig, thread), (vec![], vec![]), "apart, it runs");
    let mut rig = rig.restart(1);
    let inner = scratch(&rig).join("W").join("Sub");
    move_to(&mut rig, sub, &inner);
    turn(&mut rig, thread, 11);
    assert_eq!(
        told(&mut rig, thread),
        (vec![HELD], vec![(sub, "Sub".into())])
    );
    assert!(
        rig.actor.threads[&thread].turn.agent.is_none(),
        "no agent started"
    );
    rig.finish();
}

#[cfg(unix)]
#[test]
fn an_open_conversation_is_refused_at_its_next_turn_once_a_link_puts_a_matter_inside() {
    let mut rig = rig("held-open", TWO_ANSWERS, "exit");
    let papers = TempDir::new("held-open-papers");
    let root = dunce::canonicalize(papers.path()).unwrap();
    let thread = rig.thread();
    let sub = other_matter(&mut rig, &root, "Sub", 5);
    // Sub's folder as stored goes through a link that leads elsewhere at the launch.
    let link = root.join("Link");
    std::os::unix::fs::symlink(&root, &link).unwrap();
    move_to(&mut rig, sub, &link.join("Sub"));
    turn(&mut rig, thread, 10);
    until_idle(&mut rig, thread);
    assert_eq!(
        told(&mut rig, thread),
        (vec![], vec![]),
        "elsewhere, it runs"
    );
    // The link now leads into this matter: the rows and the fence's key are as they were.
    let into = scratch(&rig).join("W");
    crate::write::create_dir_all(&into.join("Sub")).unwrap();
    std::fs::remove_file(&link).unwrap();
    std::os::unix::fs::symlink(&into, &link).unwrap();
    assert!(live(&rig.actor, thread), "still open");
    turn(&mut rig, thread, 11);
    assert_eq!(
        told(&mut rig, thread),
        (vec![HELD], vec![(sub, "Sub".into())])
    );
    assert!(notice_then_failure(&mut rig, thread));
    rig.run_until("the agent stopped", |actor| !live(actor, thread));
    rig.finish();
}

#[test]
fn the_inner_matters_conversation_runs() {
    let mut rig = rig("held-inner", TWO_ANSWERS, "exit");
    let papers = TempDir::new("held-inner-papers");
    let sub = other_matter(&mut rig, papers.path(), "Sub", 5);
    let inner = scratch(&rig).join("W").join("Sub");
    move_to(&mut rig, sub, &inner);
    let command = HostCommand::CreateThread {
        matter: sub,
        agent: AgentId::new("fake-agent").unwrap(),
        config: None,
    };
    let CommandOutcome::ThreadCreated { thread } = rig.dispatch(6, command).unwrap() else {
        panic!("no thread");
    };
    turn(&mut rig, thread, 10);
    assert_eq!(told(&mut rig, thread), (vec![], vec![]));
    assert!(live(&rig.actor, thread), "the inner matter's agent runs");
    rig.finish();
}

#[test]
fn a_matter_whose_folder_only_starts_with_the_same_letters_refuses_nothing() {
    let mut rig = rig("held-prefix", TWO_ANSWERS, "exit");
    let papers = TempDir::new("held-prefix-papers");
    let thread = rig.thread();
    let acme2 = other_matter(&mut rig, papers.path(), "Acme2", 5);
    let mut beside = scratch(&rig).into_os_string();
    beside.push("2");
    move_to(&mut rig, acme2, &PathBuf::from(beside).join("W"));
    turn(&mut rig, thread, 10);
    assert_eq!(told(&mut rig, thread), (vec![], vec![]));
    assert!(live(&rig.actor, thread));
    rig.finish();
}

/// Where names fold (macOS, Windows), the inner folder spelled in another case is the same.
#[cfg(any(target_os = "macos", windows))]
#[test]
fn a_case_variant_spelling_of_the_inner_folder_is_refused() {
    let mut rig = rig("held-case", TWO_ANSWERS, "exit");
    let papers = TempDir::new("held-case-papers");
    let thread = rig.thread();
    let sub = other_matter(&mut rig, papers.path(), "Sub", 5);
    let inner = scratch(&rig).join("W").join("Sub");
    crate::write::create_dir_all(&inner).unwrap();
    let upper = PathBuf::from(inner.to_string_lossy().to_uppercase());
    let row = &mut rig.actor.matters.get_mut(&sub).unwrap().row;
    row.folder_path = upper.to_string_lossy().into_owned();
    turn(&mut rig, thread, 10);
    assert_eq!(
        told(&mut rig, thread),
        (vec![HELD], vec![(sub, "Sub".into())])
    );
    rig.finish();
}
