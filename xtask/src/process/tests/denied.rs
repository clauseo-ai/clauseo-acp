//! Tests of a stop the OS denies (EPERM). macOS answers EPERM, not ESRCH, to a signal for a
//! process group whose processes have all exited and are not yet reaped, which happens to a
//! command's group between its exit and its reap. The run tests use `running::STOP_HOOK` to make
//! a stop fail that way, since the window cannot be hit on purpose.

use super::super::running::{Finish, Limits, STOP_HOOK, Stop, StopHook, Tree};
use super::tree::{Grandchild, SHORT_WAIT, WRAPPER, start};
use super::*;
use nix::errno::Errno;

/// The limit of a run the tests let time out; the tree is whole before it starts.
const SHORT_LIMIT: Duration = Duration::from_millis(50);

/// Runs `script` with `limits`, each stop going through `hook`, and returns how the run
/// ended and what the tree printed after its group.
fn run_with(script: &str, limits: Limits, hook: StopHook) -> (io::Result<Finish>, Vec<u8>) {
    let (running, reader) = start(script, limits);
    let grandchild = Grandchild::of(reader);
    STOP_HOOK.set(Some(hook));
    let finish = running.finish(&mut |_, _| {});
    STOP_HOOK.set(None);
    (finish, grandchild.gone())
}

/// A hook that sends each stop and then says SIGTERM was denied, as macOS does when the group
/// held only zombies.
fn sigterm_denied() -> StopHook {
    Box::new(|stop, tree| {
        let sent = match stop {
            Stop::Terminate => tree.terminate(),
            Stop::Kill => tree.kill(),
        };
        if stop == Stop::Terminate {
            sent?;
            return Err(io::Error::from(Errno::EPERM));
        }
        sent
    })
}

#[test]
fn a_stop_denied_before_the_command_exits_is_no_error_once_it_exits() {
    let (finish, rest) = run_with(WRAPPER, Limits::of(SHORT_LIMIT), sigterm_denied());
    assert!(matches!(finish, Ok(Finish::TimedOut)), "{finish:?}");
    assert_eq!(rest, b"");
}

#[test]
fn a_stop_denied_after_the_command_exits_is_no_error() {
    let script = r#"perl -e '$SIG{TERM} = sub { print "term\n"; exit 0 };
$| = 1; print getpgrp(), "\n"; sleep 300' &"#;
    let (finish, rest) = run_with(script, Limits::of(TEST_TIMEOUT), sigterm_denied());
    assert!(
        matches!(&finish, Ok(Finish::Exited(status)) if status.success()),
        "{finish:?}"
    );
    assert_eq!(String::from_utf8_lossy(&rest), "term\n");
}

#[test]
fn a_stop_denied_to_a_command_that_never_exits_is_the_error() {
    let limits = Limits {
        grace: SHORT_WAIT,
        drain: SHORT_WAIT,
        ..Limits::of(SHORT_LIMIT)
    };
    // Nothing is sent, so only the run's last SIGKILL (`Tree::finish`) ends the tree.
    let hook: StopHook = Box::new(|_, _| Err(io::Error::from(Errno::EPERM)));
    let (finish, rest) = run_with(WRAPPER, limits, hook);
    let error = finish.expect_err("the command never exited");
    assert_eq!(
        error.to_string(),
        "`sh` could not be stopped: Operation not permitted (os error 1)"
    );
    assert_eq!(rest, b"");
}

/// Makes a process group that holds only a zombie: the perl's child moves to a group of its own
/// and exits; the perl, which does not reap it, prints the group once the child's end of a pipe
/// has closed, which happens only at its exit.
#[cfg(target_os = "macos")]
const ZOMBIE_GROUP: &str = r#"perl -e 'pipe(my $r, my $w) or die "pipe: $!";
defined(my $pid = fork) or die "fork: $!";
if (!$pid) { close $r; setpgrp(0, 0); exit 0 }
close $w; <$r>; $| = 1; print "$pid\n"; sleep 300'"#;

#[cfg(target_os = "macos")]
#[test]
fn macos_denies_a_signal_to_a_group_of_zombies() {
    let (running, reader) = start(ZOMBIE_GROUP, Limits::of(TEST_TIMEOUT));
    let grandchild = Grandchild::of(reader);
    let group = nix::unistd::Pid::from_raw(grandchild.group());
    let term = nix::sys::signal::killpg(group, nix::sys::signal::Signal::SIGTERM);
    let probe = nix::sys::signal::killpg(group, None);
    drop(running);
    assert_eq!((term, probe), (Err(Errno::EPERM), Err(Errno::EPERM)));
    assert_eq!(grandchild.gone(), b"");
}
