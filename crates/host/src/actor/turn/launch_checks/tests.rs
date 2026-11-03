//! The session folders' lifetimes: a launch removes only its own run folder, the start-up sweep
//! sets the earlier run's folders aside and deletes them in the background, and nothing is made
//! or swept through a link.

use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};
use std::time::Duration;

use clauseo_protocol::{MatterId, ThreadId, Timestamp};

use super::{RunFolders, RunSweep};
use crate::ManualClock;
use crate::agent::codex::prepare::{PREPARE_WAIT, off_actor_or_undo};
use crate::config::SessionKey;
use crate::pipeline::tests::support::id;
use crate::write;

/// A fresh data folder under the temp folder for `test`.
fn scratch(test: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("clauseo-run-{test}-{}", std::process::id()));
    write::remove_dir_all(&root).unwrap();
    write::create_dir_all(&root).unwrap();
    root
}

fn folders(data: &Path, launch: u64) -> RunFolders {
    RunFolders::new(
        data,
        SessionKey {
            matter: id::<MatterId>(7),
            thread: id::<ThreadId>(8),
            launch,
        },
    )
}

/// The names under `folder`, sorted.
fn names(folder: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(folder)
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|entry| entry.file_name().into_string().ok())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// B1: a prep the actor stopped waiting for makes its folders after the newer launch made its
/// own, and takes back only its own; the newer launch's folders and files stay.
#[test]
fn a_stale_prep_never_removes_the_newer_launch_folder() {
    let data = scratch("stale");
    let sweep = RunSweep::default();
    let (stale, newer) = (folders(&data, 1), folders(&data, 2));
    let clock = ManualClock::new(Timestamp::new(1_790_000_000_000).unwrap());
    let (entered, started) = async_channel::unbounded::<()>();
    let (release, blocked) = mpsc::channel::<()>();
    let (undone, was_undone) = mpsc::channel::<()>();
    let late = {
        let (data, sweep, stale) = (data.clone(), sweep.clone(), stale.clone());
        off_actor_or_undo(
            Arc::new(clock.clone()),
            move || {
                let _told = entered.send_blocking(());
                let _released = blocked.recv();
                stale.make(&data, &sweep)?;
                Ok(stale)
            },
            move |late: RunFolders| {
                late.remove();
                let _told = undone.send(());
            },
        )
    };
    let move_on = async {
        started.recv().await.unwrap();
        assert_eq!(clock.next_armed().await, PREPARE_WAIT);
        clock.advance(PREPARE_WAIT);
    };
    let (result, ()) = futures::executor::block_on(futures::future::join(late, move_on));
    assert!(result.is_err(), "the actor gave up on the stale prep");
    newer.make(&data, &sweep).unwrap();
    write::write_file(&newer.work.join("draft.md"), b"live").unwrap();
    release.send(()).unwrap();
    was_undone
        .recv_timeout(Duration::from_secs(5))
        .expect("the stale prep undoes its own folders");
    assert!(!stale.root.exists(), "the stale launch's folder is gone");
    assert!(newer.work.join("draft.md").exists());
    assert!(newer.browser.is_dir());
    assert!(newer.notes.is_dir(), "the shared notes folder stays");
    // The actor's own removal of a superseded answer takes only that launch's folder too.
    stale.make(&data, &sweep).unwrap();
    stale.remove();
    assert!(!stale.root.exists());
    assert!(newer.work.join("draft.md").exists());
    write::remove_dir_all(&data).unwrap();
}

/// N3: the first launch finds `run` set aside in one rename, makes its folders at once, and the
/// earlier run's folders (and a `run.old-*` an earlier start left) are deleted in the
/// background; a link of that name is not followed.
#[test]
fn the_start_up_sweep_sets_run_aside_and_deletes_it_in_the_background() {
    let data = scratch("sweep");
    let leftover = data.join("run").join("old-thread-3").join("work");
    write::create_dir_all(&leftover).unwrap();
    write::write_file(&leftover.join("f.md"), b"left").unwrap();
    write::create_dir_all(&data.join("run.old-0").join("x")).unwrap();
    let sweep = RunSweep::default();
    let live = folders(&data, 4);
    live.make(&data, &sweep).unwrap();
    assert_eq!(
        names(&data.join("run")),
        [live.key.id()],
        "the leftover is out of run at once"
    );
    sweep.join_delete();
    assert!(
        names(&data)
            .iter()
            .all(|name| !name.starts_with("run.old-")),
        "the set-aside folders are deleted"
    );
    assert!(live.work.is_dir(), "the live launch's folders stay");
    // Once per host start: a second launch sweeps nothing.
    let second = folders(&data, 5);
    second.make(&data, &sweep).unwrap();
    assert!(live.work.is_dir());
    write::remove_dir_all(&data).unwrap();
}

/// N2: a `run` or `agent-state` that is a link is neither swept nor made through: the launch
/// fails, and the link's target is left as it was.
#[cfg(unix)]
#[test]
fn nothing_is_swept_or_made_through_a_linked_run_or_agent_state() {
    let data = scratch("linked");
    let outside = scratch("linked-outside");
    write::create_dir_all(&outside.join("keep")).unwrap();
    write::symlink(&outside, &data.join("run")).unwrap();
    write::create_dir_all(&data.join("run.old-9-target")).unwrap();
    write::symlink(&outside, &data.join("run.old-9")).unwrap();
    let sweep = RunSweep::default();
    let launch = folders(&data, 1);
    let error = launch.make(&data, &sweep).unwrap_err();
    assert!(error.to_string().contains("link"), "{error}");
    sweep.join_delete();
    assert_eq!(
        names(&outside),
        ["keep"],
        "the linked run is left as it was"
    );
    assert!(data.join("run").is_symlink());

    let state_data = scratch("linked-state");
    let state_outside = scratch("linked-state-outside");
    write::symlink(&state_outside, &state_data.join("agent-state")).unwrap();
    let error = folders(&state_data, 2)
        .make(&state_data, &RunSweep::default())
        .unwrap_err();
    assert!(error.to_string().contains("link"), "{error}");
    assert!(
        names(&state_outside).is_empty(),
        "nothing is made through the link"
    );
    assert!(
        names(&state_data.join("run")).is_empty(),
        "the failed launch takes its run folder back"
    );
    for folder in [&data, &outside, &state_data, &state_outside] {
        write::remove_dir_all(folder).unwrap();
    }
}
