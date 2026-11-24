//! The keychain Ask child's bookkeeping in `system.rs`: the quit's stop, its environment, and
//! (on Unix, with a real child) the limit and the stop killing it.

use futures::executor::block_on;

use super::system::AskStop;

#[test]
fn the_quit_stops_a_registered_child() {
    let stop = AskStop::new();
    let fired = stop.register();
    stop.stop();
    assert_eq!(block_on(fired), Ok(()));
}

#[test]
fn a_child_registered_after_the_quit_is_stopped_at_once() {
    let stop = AskStop::new();
    stop.stop();
    // Its sender is gone: the receiver resolves, which stops the child as a fire does.
    assert!(block_on(stop.register()).is_err());
}

#[test]
fn a_child_that_ends_unstopped_leaves_the_next_one_stoppable() {
    let stop = AskStop::new();
    drop(stop.register());
    let second = stop.register();
    stop.stop();
    assert_eq!(block_on(second), Ok(()));
}

#[test]
fn the_child_gets_home_and_nothing_else() {
    let env = super::system::child_env();
    assert!(env.keys().all(|name| name == "HOME"), "{env:?}");
    assert_eq!(
        env.get(std::ffi::OsStr::new("HOME")),
        std::env::var_os("HOME").as_ref()
    );
}

#[cfg(unix)]
mod real {
    use std::ffi::OsStr;
    use std::path::Path;
    use std::time::Duration;

    use futures::channel::oneshot;

    use super::super::system::{ChildOpts, Ran, run};

    /// `sleep 30` as the child: it never ends by itself within a test.
    fn sleeper(opts: ChildOpts<'_>) -> (Ran, Duration) {
        let began = crate::open::now();
        let ran = run(Path::new("/bin/sleep"), &[OsStr::new("30")], opts).unwrap();
        (ran, began.elapsed())
    }

    #[test]
    fn past_its_limit_the_child_is_killed() {
        let opts = ChildOpts {
            limit: Some(Duration::from_millis(200)),
            ..ChildOpts::default()
        };
        let (ran, took) = sleeper(opts);
        assert_eq!(ran, None);
        assert!(took < Duration::from_secs(10), "{took:?}");
    }

    #[test]
    fn the_stop_kills_the_child() {
        let (fire, fired) = oneshot::channel();
        let stopper = std::thread::spawn(move || {
            std::thread::park_timeout(Duration::from_millis(200));
            fire.send(()).unwrap();
        });
        let opts = ChildOpts {
            stop: Some(fired),
            ..ChildOpts::default()
        };
        let (ran, took) = sleeper(opts);
        stopper.join().unwrap();
        assert_eq!(ran, None);
        assert!(took < Duration::from_secs(10), "{took:?}");
    }

    #[test]
    fn the_child_gets_only_the_environment_given() {
        let env = [("HOME".into(), "/nowhere".into())].into_iter().collect();
        let opts = ChildOpts {
            env: Some(env),
            ..ChildOpts::default()
        };
        let ran = run(Path::new("/usr/bin/env"), &[], opts).unwrap();
        let (code, out) = ran.unwrap();
        assert_eq!(code, Some(0));
        assert_eq!(String::from_utf8(out).unwrap(), "HOME=/nowhere\n");
    }
}
