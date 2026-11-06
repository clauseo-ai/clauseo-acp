//! `stop_on_exit`'s order and bounds (`RunEvent::Exit`: macOS's quit, a Windows logoff), the
//! `ExitRequested` stopper's last steps, and the second start during a quit. The order is
//! tested on `stop_on_exit_with` with recording steps, since the mock runtime's `hide` does
//! nothing and its windows always answer visible (tauri 2.12.4 `test/mock_runtime.rs`); the
//! app's own wiring runs on the mock runtime below.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use clauseo_host::{Downloaded, HostExit, Opaque};
use tauri::test::{MockRuntime, mock_builder};
use tauri::{Manager as _, RunEvent, WebviewUrl, WebviewWindowBuilder};

use super::super::{
    Exit, ExitWait, after_host_unless_exiting, on_exit_requested, stop_on_exit, stop_on_exit_with,
};
use super::InstallSteps;
use crate::bridge::lock;
use crate::rust_only::PluginRustOnly as _;
use crate::update::install::{self, After, Watch};

/// Far past any test's run: a wait that reaches it is a hang.
const LONG: Duration = Duration::from_secs(120);

/// A download, as the host hands it back.
fn downloaded() -> Downloaded {
    Downloaded {
        version: "0.4.1".to_owned(),
        path: PathBuf::from("updates/0.4.1/Clauseo-setup.exe"),
        handle: Opaque::new(()),
    }
}

/// The windows hide before anything else, the flag is set before the stopper starts (so before
/// any wait), and the wait ends when the stopper marks the stop ended.
#[test]
fn the_windows_hide_and_the_flag_is_set_before_the_stopper_starts() {
    let exit = Arc::new(Exit::default());
    let steps = Arc::new(Mutex::new(Vec::new()));
    let (hide_steps, idle_steps, start_steps) =
        (Arc::clone(&steps), Arc::clone(&steps), Arc::clone(&steps));
    let mut stopper = None;
    let waited = stop_on_exit_with(
        &exit,
        LONG,
        || lock(&hide_steps).push("hide"),
        || {
            lock(&idle_steps).push("idle");
            false
        },
        |stopper_exit| {
            let flag = stopper_exit.exiting_on_main.load(Ordering::Acquire);
            lock(&start_steps).push(if flag { "start, flag set" } else { "start" });
            stopper = Some(std::thread::spawn(move || stopper_exit.mark_stopped()));
            true
        },
        || lock(&steps).push("flush"),
    );
    assert_eq!(waited, ExitWait::Stopped);
    assert_eq!(*lock(&steps), ["hide", "idle", "start, flag set"]);
    stopper.unwrap().join().unwrap();
}

/// A stopper already running (a quit signal or a closed window came first): the exit starts
/// none, asks nothing, and waits on the condvar until that stopper marks the stop ended. Two
/// exits at once start one stopper between them.
#[test]
fn an_exit_during_a_running_stop_waits_for_it_and_starts_no_second() {
    let exit = Arc::new(Exit::default());
    exit.stopping.store(true, Ordering::Release);
    // The running stopper ends once the exit has begun: its hide says so.
    let (begun, began) = mpsc::channel();
    let marker = Arc::clone(&exit);
    let running = std::thread::spawn(move || {
        began.recv().unwrap();
        marker.mark_stopped();
    });
    let waited = stop_on_exit_with(
        &exit,
        LONG,
        || begun.send(()).unwrap(),
        || panic!("only the first stopper asks whether anything runs"),
        |_exit| panic!("a stopper already runs"),
        || panic!("the running stopper flushes the log"),
    );
    assert_eq!(waited, ExitWait::Stopped);
    assert!(exit.exiting_on_main.load(Ordering::Acquire));
    running.join().unwrap();

    let exit = Arc::new(Exit::default());
    let starts = Arc::new(AtomicUsize::new(0));
    let racers: Vec<_> = (0..2)
        .map(|_| {
            let exit = Arc::clone(&exit);
            let starts = Arc::clone(&starts);
            std::thread::spawn(move || {
                stop_on_exit_with(
                    &exit,
                    LONG,
                    || {},
                    || false,
                    |stopper_exit| {
                        starts.fetch_add(1, Ordering::AcqRel);
                        std::thread::spawn(move || stopper_exit.mark_stopped());
                        true
                    },
                    || {},
                )
            })
        })
        .collect();
    for racer in racers {
        assert_eq!(racer.join().unwrap(), ExitWait::Stopped);
    }
    assert_eq!(starts.load(Ordering::Acquire), 1);
}

/// A stopper that never ends: the exit gives up at its bound (a short one here; the app's is
/// `EXIT_STOP_WAIT`, 13 s), flushes the log, and returns.
#[test]
fn a_stop_that_does_not_end_is_given_up_on_at_the_bound() {
    let exit = Arc::new(Exit::default());
    let flushed = AtomicUsize::new(0);
    let waited = stop_on_exit_with(
        &exit,
        Duration::from_millis(100),
        || {},
        || false,
        // Started, and never marks the stop ended.
        |_exit| true,
        || {
            flushed.fetch_add(1, Ordering::AcqRel);
        },
    );
    assert_eq!(waited, ExitWait::TimedOut);
    assert_eq!(flushed.load(Ordering::Acquire), 1);
    assert!(!exit.stopped.load(Ordering::Acquire));
}

/// Nothing runs (a Windows logoff with the app idle): the exit hides the windows, starts no
/// stopper and waits for nothing; the log is flushed. One that stopped already does nothing.
#[test]
fn an_idle_app_exits_without_a_stop_or_a_wait() {
    let exit = Arc::new(Exit::default());
    let steps = Mutex::new(Vec::new());
    let waited = stop_on_exit_with(
        &exit,
        LONG,
        || lock(&steps).push("hide"),
        || true,
        |_exit| panic!("nothing runs: no stopper"),
        || lock(&steps).push("flush"),
    );
    assert_eq!(waited, ExitWait::Idle);
    assert_eq!(*lock(&steps), ["hide", "flush"]);

    let stopped = Arc::new(Exit::default());
    stopped.mark_stopped();
    let waited = stop_on_exit_with(
        &stopped,
        LONG,
        || panic!("stopped already: the windows are going"),
        || panic!("stopped already"),
        |_exit| panic!("stopped already"),
        || panic!("stopped already"),
    );
    assert_eq!(waited, ExitWait::AlreadyStopped);
}

/// The host stopped for an update while `RunEvent::Exit` holds the main thread: the stopper's
/// `after_host` installs nothing (the install waits on the main thread).
/// Without the flag the install runs as before.
#[test]
fn the_flag_skips_the_install() {
    for flag in [true, false] {
        let exit = Exit::default();
        exit.exiting_on_main.store(flag, Ordering::Release);
        let mut steps = InstallSteps::default();
        let mut after = None;
        after_host_unless_exiting(
            &exit,
            HostExit::Install(downloaded()),
            true,
            |ended, agents_stopped| {
                after = Some(install::after_host(
                    ended,
                    agents_stopped,
                    &mut steps,
                    Watch::INSTALL,
                ));
            },
        );
        if flag {
            assert_eq!(after, None);
            assert!(steps.0.is_empty(), "{:?}", steps.0);
        } else {
            assert_eq!(after, Some(After::Restart));
            assert!(steps.0.contains(&"install"), "{:?}", steps.0);
        }
    }
}

/// `ExitRequested`: the first request saves the window state (on the main thread, where the
/// arm runs), then hides the windows, then starts the stopper; a later one does none of it. The
/// stopper and the update path have no save to call: `stop_steps` takes none, and the install's
/// `Steps` has none (`the_flag_skips_the_install` and `update/tests.rs` record every step).
#[test]
fn an_exit_request_saves_then_hides_then_starts_the_stopper_once() {
    let exit = Exit::default();
    let steps = Mutex::new(Vec::new());
    let saves = AtomicUsize::new(0);
    for _request in 0..2 {
        let _first = on_exit_requested(
            &exit,
            || {
                saves.fetch_add(1, Ordering::AcqRel);
                lock(&steps).push("save");
            },
            || lock(&steps).push("hide"),
            || lock(&steps).push("stopper"),
        );
    }
    assert_eq!(*lock(&steps), ["save", "hide", "stopper"]);
    assert_eq!(saves.load(Ordering::Acquire), 1);
    assert!(exit.stopping.load(Ordering::Acquire));
}

/// An app on the mock runtime with the app's context and its main window.
fn app() -> tauri::App<MockRuntime> {
    let app = mock_builder()
        .build(crate::context::<MockRuntime>())
        .unwrap();
    WebviewWindowBuilder::new(
        &app,
        crate::MAIN_WINDOW,
        WebviewUrl::App("index.html".into()),
    )
    .build()
    .unwrap();
    app
}

/// The stopper's steps and the update path on the mock runtime with no window-state plugin, so
/// any window-state save would panic (its cache is not managed): `stop_steps` with
/// `update::after_host` for a stop to install runs to its end. The download is not the plugin's,
/// so nothing installs.
#[test]
fn the_stopper_and_the_update_path_save_no_window_state() {
    let app = app();
    let exit = Exit::default();
    let mut ended = None;
    super::super::stop_steps(app.handle(), |host, agents_stopped| {
        assert!(matches!(host, HostExit::Stopped));
        after_host_unless_exiting(
            &exit,
            HostExit::Install(downloaded()),
            agents_stopped,
            |install, stopped| {
                crate::update::after_host(app.handle(), install, stopped);
                ended = Some(stopped);
            },
        );
    });
    assert_eq!(ended, Some(true));
}

/// The app's `stop_on_exit` on the mock runtime with no host, setup or counts managed: it sets
/// the flag and returns without the 13 s wait (idle, or a stopper with nothing to stop when
/// another test of this binary holds a process). Once stopped, it does nothing.
#[test]
fn the_app_s_exit_with_nothing_managed_returns_at_once() {
    let app = app();
    let exit = Arc::new(Exit::default());
    let waited = stop_on_exit(app.handle(), &exit);
    assert!(
        matches!(waited, ExitWait::Idle | ExitWait::Stopped),
        "{waited:?}"
    );
    assert!(exit.exiting_on_main.load(Ordering::Acquire));

    let stopped = Arc::new(Exit::default());
    stopped.mark_stopped();
    assert_eq!(
        stop_on_exit(app.handle(), &stopped),
        ExitWait::AlreadyStopped
    );
    assert!(!stopped.exiting_on_main.load(Ordering::Acquire));
}

/// Tauri runs the plugins' `RunEvent::Exit` (window-state's save) before the app's callback,
/// where `stop_on_exit` hides the windows: so the save never sees them hidden. A plugin that
/// records stands in for window-state, whose save would write to the real config folder.
#[test]
fn the_plugins_exit_save_runs_before_the_app_s_exit_callback() {
    let order = Arc::new(Mutex::new(Vec::new()));
    let plugin_order = Arc::clone(&order);
    let app = mock_builder()
        .plugin_rust_only(
            tauri::plugin::Builder::<MockRuntime>::new("save-stand-in")
                .on_event(move |_app, event| {
                    if matches!(event, RunEvent::Exit) {
                        lock(&plugin_order).push("plugin save");
                    }
                })
                .build(),
        )
        .build(crate::context::<MockRuntime>())
        .unwrap();
    let window = WebviewWindowBuilder::new(
        &app,
        crate::MAIN_WINDOW,
        WebviewUrl::App("index.html".into()),
    )
    .build()
    .unwrap();
    let callback_order = Arc::clone(&order);
    app.run(move |_app, event| match event {
        // Closing the only window once the loop runs ends it: `ExitRequested`, then `Exit`.
        RunEvent::Ready => window.close().unwrap(),
        RunEvent::Exit => lock(&callback_order).push("callback hide"),
        _ => {}
    });
    assert_eq!(*lock(&order), ["plugin save", "callback hide"]);
}

/// The single-instance callback: a second start raises the window, but not while a quit stops
/// the app (its windows are hidden and it is ending). The plugin's hand-off itself needs a
/// second process (a socket, D-Bus or a named pipe), which the mock runtime has no way to
/// send, so the callback's body is called directly.
#[test]
fn a_second_start_during_the_quit_raises_nothing() {
    let app = app();
    let exit = Exit::default();
    assert!(super::super::on_second_start(app.handle(), &exit));
    exit.stopping.store(true, Ordering::Release);
    assert!(!super::super::on_second_start(app.handle(), &exit));
    assert!(app.get_webview_window(crate::MAIN_WINDOW).is_some());
}
