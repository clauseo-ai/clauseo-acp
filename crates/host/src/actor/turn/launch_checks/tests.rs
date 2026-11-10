//! The session folders' lifetimes: a launch removes only its own run folder, the start-up sweep
//! sets the earlier run's folders aside and deletes them in the background, and nothing is made
//! or swept through a link.

use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};
use std::time::Duration;

use clauseo_protocol::{ErrorCode, MatterId, ThreadId, Timestamp};

use super::{RunFolders, RunSweep};
use crate::ManualClock;
use crate::agent::codex::prepare::{Job, LAUNCH_WAIT, PREPARE_WAIT, off_actor_or_undo};
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
            Job::short("a stale prep", None),
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

/// `{matter_notes}`: a notes path whose matter folder is a link is refused, as one whose
/// `agent-state` is (above): the launch fails with `agent.launch_failed` naming the link, and
/// nothing is made through it. A notes folder made earlier at another mode is set to 0700.
#[cfg(unix)]
#[test]
fn the_notes_folder_is_private_and_never_made_through_a_linked_matter() {
    use std::os::unix::fs::PermissionsExt;

    let data = scratch("notes-matter-link");
    let outside = scratch("notes-matter-link-outside");
    let launch = folders(&data, 1);
    let matter = launch.notes.parent().unwrap().to_owned();
    write::create_dir_all(matter.parent().unwrap()).unwrap();
    write::symlink(&outside, &matter).unwrap();
    let error = launch.make(&data, &RunSweep::default()).unwrap_err();
    assert_eq!(error.code(), clauseo_protocol::ErrorCode::AgentLaunchFailed);
    assert!(error.to_string().contains("link"), "{error}");
    assert!(
        error.to_string().contains(&matter.display().to_string()),
        "{error}"
    );
    assert!(
        names(&outside).is_empty(),
        "nothing is made through the link"
    );
    assert!(!launch.root.exists(), "the run folder is taken back");

    write::remove_file(&matter).unwrap();
    write::create_dir_all(&launch.notes).unwrap();
    write::set_mode(&launch.notes, 0o755).unwrap();
    launch.make(&data, &RunSweep::default()).unwrap();
    let mode = std::fs::symlink_metadata(&launch.notes)
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o700);
    assert_eq!(
        launch.notes,
        data.join("agent-state")
            .join(id::<MatterId>(7).to_string())
            .join("notes"),
        "exactly the matter's notes folder, never a parent"
    );
    for folder in [&data, &outside] {
        write::remove_dir_all(folder).unwrap();
    }
}

/// `remove_run`'s removal takes the run folder the host made, with a link inside it, and never
/// follows the link: the file outside it survives.
#[cfg(unix)]
#[test]
fn removing_a_run_folder_never_follows_a_link_inside_it() {
    let data = scratch("remove-link");
    let outside = scratch("remove-link-outside");
    write::write_file(&outside.join("brief.md"), b"keep").unwrap();
    let launch = folders(&data, 1);
    launch.make(&data, &RunSweep::default()).unwrap();
    write::symlink(&outside, &launch.work.join("linked")).unwrap();
    write::symlink(&outside.join("brief.md"), &launch.browser.join("file-link")).unwrap();
    launch.remove();
    assert!(!launch.root.exists(), "the run folder is gone");
    assert_eq!(std::fs::read(outside.join("brief.md")).unwrap(), b"keep");
    assert!(launch.notes.is_dir(), "the notes folder stays");
    // A path that is not a run folder the host built is never removed.
    let mut stray = folders(&data, 2);
    stray.root = outside.clone();
    stray.remove();
    assert!(outside.join("brief.md").exists());
    for folder in [&data, &outside] {
        write::remove_dir_all(folder).unwrap();
    }
}

/// #365: a launch's preparation that takes longer than [`PREPARE_WAIT`] (a first Codex launch on
/// Windows, whose new files an antivirus scans) is waited for, and its answer is taken: the
/// launch goes on instead of failing as `matter.folder_missing`.
#[test]
fn a_launch_prep_slower_than_the_short_wait_is_waited_for() {
    let clock = ManualClock::new(Timestamp::new(1_790_000_000_000).unwrap());
    let (entered, started) = async_channel::unbounded::<()>();
    let (release, blocked) = mpsc::channel::<()>();
    let slow = off_actor_or_undo(
        Arc::new(clock.clone()),
        Job::launch(id::<MatterId>(7)),
        move || {
            let _told = entered.send_blocking(());
            let _released = blocked.recv();
            Ok("prepared")
        },
        drop,
    );
    let mut slow = std::pin::pin!(slow);
    // The first poll starts the work and arms the caller's timer.
    assert!(futures::FutureExt::now_or_never(slow.as_mut()).is_none());
    futures::executor::block_on(async {
        started.recv().await.unwrap();
        assert_eq!(clock.next_armed().await, LAUNCH_WAIT);
        clock.advance(PREPARE_WAIT + Duration::from_secs(1));
    });
    // Past PREPARE_WAIT, with the work not done yet: the caller still waits.
    let early = futures::FutureExt::now_or_never(slow.as_mut());
    assert!(early.is_none(), "the caller gave up: {early:?}");
    release.send(()).unwrap();
    let result = futures::executor::block_on(slow);
    assert_eq!(result.unwrap(), "prepared");
}

/// #365: a launch's preparation that has not answered by [`LAUNCH_WAIT`] fails as
/// `agent.launch_failed`, which may be retried, never as `matter.folder_missing`; what it made
/// once it answers is undone on its thread.
#[test]
fn a_launch_prep_past_its_bound_is_retryable_not_a_missing_folder() {
    let clock = ManualClock::new(Timestamp::new(1_790_000_000_000).unwrap());
    let (entered, started) = async_channel::unbounded::<()>();
    let (release, blocked) = mpsc::channel::<()>();
    let (undone, was_undone) = mpsc::channel::<&str>();
    let hung = off_actor_or_undo(
        Arc::new(clock.clone()),
        Job::launch(id::<MatterId>(7)),
        move || {
            let _told = entered.send_blocking(());
            let _released = blocked.recv();
            Ok("prepared")
        },
        move |late| {
            let _told = undone.send(late);
        },
    );
    let move_on = async {
        started.recv().await.unwrap();
        assert_eq!(clock.next_armed().await, LAUNCH_WAIT);
        clock.advance(LAUNCH_WAIT);
    };
    let (result, ()) = futures::executor::block_on(futures::future::join(hung, move_on));
    let error = result.unwrap_err();
    assert_eq!(error.code(), ErrorCode::AgentLaunchFailed);
    assert!(error.code().is_retryable());
    release.send(()).unwrap();
    assert_eq!(
        was_undone.recv_timeout(Duration::from_secs(5)),
        Ok("prepared"),
        "the late answer is undone"
    );
}

/// A Codex launch in a matter whose folder is gone still fails as `matter.folder_missing`, at
/// once and before Codex's home is made: the slow-launch change keeps the real case.
#[test]
fn a_codex_launch_in_a_missing_folder_is_folder_missing() {
    let data = scratch("missing-folder");
    let gone = data.join("matters").join("gone");
    let home = data.join("agents").join("codex-acp").join("home");
    let clock = ManualClock::new(Timestamp::new(1_790_000_000_000).unwrap());
    let prep = {
        let (data, home) = (data.clone(), home.clone());
        off_actor_or_undo(
            Arc::new(clock),
            Job::launch(id::<MatterId>(7)),
            move || {
                let check = crate::agent::folder_support::FolderCheck {
                    matter: Some(id::<MatterId>(7)),
                    source: crate::agent::folder_support::CheckSource::Windows,
                };
                crate::agent::codex::prepare_launch(&check, &gone, &data, &home)
            },
            drop,
        )
    };
    let error = futures::executor::block_on(prep).unwrap_err();
    assert_eq!(error.code(), ErrorCode::MatterFolderMissing);
    assert!(!home.exists(), "nothing is made for a missing folder");
    write::remove_dir_all(&data).unwrap();
}

/// A prep nobody waits for any more frees what it claimed: no agent was started, so there is
/// nothing to wait for, and its parts are not held.
#[test]
fn a_late_prep_frees_its_claims() {
    let data = scratch("undo-late");
    let run = folders(&data, 9);
    run.make(&data, &RunSweep::default()).unwrap();
    let session = Arc::new(
        crate::browser::session::BrowserSession::claim(
            "undo-late",
            #[cfg(unix)]
            None,
        )
        .unwrap(),
    );
    let alive = Arc::downgrade(&session);
    let held = super::HeldRuns::default();
    run.lease.keep(
        held.clone(),
        super::straggler::Parts {
            browser: Some(session),
            ..super::straggler::Parts::default()
        },
    );
    let root = run.root.clone();
    super::undo_late(super::Prepared {
        stored: data.clone(),
        codex: None,
        claude_home: None,
        folder: None,
        run,
        skills_written: None,
        packs_skipped: Vec::new(),
    });
    assert!(alive.upgrade().is_none(), "its claims were freed");
    assert_eq!(held.count(), 0, "nothing is held");
    assert!(!root.exists(), "its run folder is gone");
    write::remove_dir_all(&data).unwrap();
}

/// The shell browser's environment for a session, built from its run folders as the launch
/// builds them, against that session's Codex fence as the launch lists it: `AGENT_BROWSER_CONFIG`,
/// `_DOWNLOAD_PATH` and `_PROFILE` and this matter's brief all writable, to an ordinary command
/// and to an escalated one (agent-browser's always is), the `agent-browser` copy in
/// `<data>/tools` readable and not writable to an ordinary command; another matter's session's
/// three and its brief denied to both, and so is `<data>/browser`.
#[test]
fn the_shell_browser_env_is_reachable_under_its_own_codex_fence_only() {
    use std::collections::BTreeMap;
    use std::ffi::OsString;

    use crate::agent::codex_fence::tests::{escalated_access, session_access};
    use crate::agent::codex_fence::{Session, keys, list};
    use crate::agent::loads::browser::{ShellInputs, shell_env};

    let root = scratch("abfence-combined");
    let locations = clauseo_settings::Locations::under_root(&root);
    let data = locations.data_dir().to_path_buf();
    let sweep = RunSweep::default();
    let run = folders(&data, 1);
    let other = RunFolders::new(
        &data,
        SessionKey {
            matter: id::<MatterId>(9),
            thread: id::<ThreadId>(10),
            launch: 1,
        },
    );
    for folders in [&run, &other] {
        folders.make(&data, &sweep).unwrap();
        write::create_dir_all(&folders.browser_profile).unwrap();
        write::create_dir_all(&folders.downloads).unwrap();
    }
    let listed = list(&data, &run.root, &run.notes);
    let codex_home = locations.agent_home("codex-acp");
    let session = Session {
        run_root: &run.root,
        notes: &run.notes,
        codex_home: &codex_home,
        listed: &listed,
    };
    let fence = keys(&locations, session, false);
    let own = [run.root.clone(), run.notes.clone()];
    let env_of = |run: &RunFolders| -> BTreeMap<OsString, OsString> {
        let id = run.key.id();
        shell_env(&ShellInputs {
            session: &id,
            config: &run.browser_config,
            downloads: &run.downloads,
            profile: &run.browser_profile,
            executable: Path::new("/opt/chrome"),
        })
    };
    let names = [
        "AGENT_BROWSER_CONFIG",
        "AGENT_BROWSER_DOWNLOAD_PATH",
        "AGENT_BROWSER_PROFILE",
    ];
    let paths = |run: &RunFolders| -> Vec<PathBuf> {
        let env = env_of(run);
        let mut paths: Vec<PathBuf> = names
            .iter()
            .map(|name| PathBuf::from(&env[&OsString::from(name)]))
            .collect();
        paths.push(run.browser_profile.join("Default").join("Cookies"));
        paths.push(run.notes.join("matter-brief.md"));
        paths
    };
    for path in paths(&run) {
        assert_eq!(
            session_access(&fence, &own, &path),
            "write",
            "{}",
            path.display()
        );
        assert_eq!(
            escalated_access(&fence, &path),
            "write",
            "{}",
            path.display()
        );
    }
    for path in &paths(&run)[..3] {
        assert!(path.starts_with(&run.agent_browser), "{}", path.display());
    }
    let tools = data.join(clauseo_profile::paths::TOOLS_DIR);
    let copy = tools.join(clauseo_profile::paths::AGENT_BROWSER_COMMAND);
    assert_eq!(session_access(&fence, &own, &copy), "read");
    let app_wide = data
        .join(crate::browser::CONFIG_DIR)
        .join(crate::browser::CONFIG_FILE);
    for path in paths(&other).into_iter().chain([app_wide]) {
        assert_eq!(
            session_access(&fence, &own, &path),
            "deny",
            "{}",
            path.display()
        );
        assert_eq!(
            escalated_access(&fence, &path),
            "deny",
            "{}",
            path.display()
        );
    }
    run.remove();
    other.remove();
    write::remove_dir_all(&root).unwrap();
}

/// The shell tool's folders and config are made in the run folder's own `agent-browser`
/// folder, the config `{}` and private, a planted one replaced at the next launch.
#[test]
fn the_shell_browser_folders_and_config_are_made_in_the_own_agent_browser_folder() {
    let data = scratch("agent-browser");
    let mut run = folders(&data, 1);
    run.make(&data, &RunSweep::default()).unwrap();
    run.chrome = Some(PathBuf::from("/opt/chrome"));
    super::browser::shell_folders(&mut run, &data);
    assert!(run.chrome.is_some());
    assert_eq!(run.agent_browser, run.root.join("agent-browser"));
    assert!(run.downloads.is_dir() && run.browser_profile.is_dir());
    assert_eq!(std::fs::read(&run.browser_config).unwrap(), b"{}\n");
    assert_eq!(
        names(&run.agent_browser),
        ["config.json", "downloads", "profile"]
    );
    assert!(!data.join(crate::browser::CONFIG_DIR).exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&run.browser_config), 0o600);
        assert_eq!(mode(&run.agent_browser), 0o700);
    }
    write::write_file(&run.browser_config, br#"{"provider":"browserbase"}"#).unwrap();
    super::browser::shell_folders(&mut run, &data);
    assert_eq!(std::fs::read(&run.browser_config).unwrap(), b"{}\n");
    run.remove();
    write::remove_dir_all(&data).unwrap();
}

/// The session's temp folder (D-023) is made with its run folder and goes with it.
#[test]
fn the_temp_folder_is_made_in_the_run_folder_and_goes_with_it() {
    let data = scratch("temp-folder");
    let launch = folders(&data, 1);
    launch.make(&data, &RunSweep::default()).unwrap();
    let tmp = crate::agent::temp::temp_dir(&launch.root);
    assert_eq!(tmp, data.join("run").join(launch.key.id()).join("tmp"));
    assert!(tmp.is_dir());
    write::write_file(&tmp.join("module.pcm"), b"cache").unwrap();
    launch.remove();
    assert!(
        !launch.root.exists(),
        "the temp folder went with the run folder"
    );
    write::remove_dir_all(&data).unwrap();
}
