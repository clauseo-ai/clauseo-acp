//! H3's remainder (`live.rs`, `Run::below`): a scan of a folder made or moved in stops at its
//! budget, and the folder goes whole only when a pool user (the agent) owns it. The stand-in
//! volume says who owns what, so these run on Unix.

use std::time::Duration;

use super::live::{reported, ticking, watched};
use super::posix::Posix;
use super::{exit, world};
use crate::SetAsideWhat;
use crate::windows::set_aside::live::Ending;

/// H3: a scan of a big tree the agent made (a pool user owns it) ends the session once past its
/// budget and stops there: the whole folder goes at once, its repository with it, before the
/// scan reaches the rest.
#[test]
fn past_its_budget_a_folder_the_agent_made_is_set_aside_whole_without_scanning_on() {
    let (w, root, data) = world("live-budget");
    let watch = watched(&root, &data);
    for at in 0..40 {
        w.dir.file(&format!("matter/big/f{at}/keep.txt"), "");
    }
    w.dir
        .file("matter/big/f5/.git/HEAD", "ref: refs/heads/main\n");
    let agents = Posix::new(true);
    agents.foreign(&root.join("big"));

    let step = Duration::from_millis(500);
    let hooks = ticking(step);
    let live = watch.changed(&agents, &reported(&["big"]), exit(), &hooks);
    assert_eq!(*hooks.ends.borrow(), [Ending::Slow, Ending::Found]);
    let last = live.moved.last().expect("the folder moved");
    assert_eq!(
        (last.entry.from.clone(), last.what),
        (root.join("big"), SetAsideWhat::UncheckedFolder)
    );
    assert!(!root.join("big").exists());
    let readings = hooks.clock.get().as_millis() / step.as_millis();
    assert!(
        readings < 20,
        "the scan went on: {readings} clock readings for 40 folders"
    );
}

/// The same past its budget in a folder someone else owns (the lawyer dragging a big client
/// folder in while the agent ran): the session ends, and the folder stays where it is, for the
/// check after the session.
#[test]
fn past_its_budget_a_folder_the_agent_didnt_make_stays_and_the_session_ends() {
    let (w, root, data) = world("live-budget-lawyers");
    let watch = watched(&root, &data);
    for at in 0..40 {
        w.dir.file(&format!("matter/big/f{at}/keep.txt"), "");
    }
    let lawyers = Posix::new(true);

    let hooks = ticking(Duration::from_millis(500));
    let live = watch.changed(&lawyers, &reported(&["big"]), exit(), &hooks);
    assert_eq!(*hooks.ends.borrow(), [Ending::Slow]);
    assert!(live.moved.is_empty() && live.failed.is_empty());
    assert!(root.join("big/f0/keep.txt").exists());
}
