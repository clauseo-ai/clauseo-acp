//! Tests of how a command's tree is stopped: at its limit, when its run is dropped or panics,
//! after it exits, with SIGKILL when it ignores SIGTERM, and when a thread of its run cannot
//! start. Most run a wrapper that starts a grandchild, prints its own pid (its group's id) and
//! waits, as `ui/app/scripts/buildtime-proc.test.mjs` does. The grandchild holds the write end of
//! a pipe the test reads, so the test sees EOF only once the wrapper and the grandchild are both
//! gone.
//! Nothing sleeps: the test blocks on that pipe or on the run. `leader/tests.rs` uses them too.
//!
//! A run's limit counts from the start of `Running::finish`, and these tests read the wrapper's
//! line before they call it, so a stop at the limit always meets the whole tree, however loaded
//! the machine. That is why their limits can be short.

use super::super::running::{self, Finish, Limits, Running, Streams};
use super::*;
use crate::clock::timed;
use nix::errno::Errno;
use nix::sys::signal::{Signal, kill, killpg};
use nix::unistd::Pid;
use std::cell::{Cell, RefCell};
use std::io::{BufRead, BufReader, PipeReader, Read};
use std::panic::{self, AssertUnwindSafe};
use std::process::Command;
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver};
use std::thread;

/// Starts a grandchild that holds stdout, prints its own pid, which is its group's id, and waits.
pub(in crate::process) const WRAPPER: &str = "sleep 300 & echo $$; wait";

/// `WRAPPER`, but on SIGTERM it prints `term` and exits. So with SIGTERM first its run ends long
/// before `KILL_GRACE`, and with SIGKILL alone it prints nothing. The grandchild is a perl that
/// prints the group's id itself once it runs: a `sleep` the trapping shell has forked but not yet
/// started can catch the SIGTERM in the shell's trap and lose it, and then outlives it.
const REPORTS_SIGTERM: &str = r#"trap 'echo term; exit 0' TERM
perl -e '$| = 1; print getpgrp(), "\n"; sleep 300' & wait"#;

/// The limit of a run the tests let time out. The tree is whole before it starts (see the top of
/// this file), so it only has to be short.
const SHORT_LIMIT: Duration = Duration::from_millis(50);

/// The grace of the test that waits it out, and the drain of the test that waits that out.
pub(in crate::process) const SHORT_WAIT: Duration = Duration::from_millis(200);

/// How long a test waits for EOF, or for a run to end, before it fails: far longer than the
/// `KILL_GRACE` and drain a stop takes.
pub(in crate::process) const SURVIVOR: Duration = Duration::from_secs(20);

/// The tree of a wrapper that printed its group's id, and the EOF of the pipe it holds.
pub(in crate::process) struct Grandchild {
    group: i32,
    eof: Receiver<io::Result<Vec<u8>>>,
}

impl Grandchild {
    /// The id of the wrapper's group.
    pub(in crate::process) fn group(&self) -> i32 {
        self.group
    }

    /// Reads the group the wrapper printed on `reader`'s first line, then reads the rest on
    /// another thread.
    pub(in crate::process) fn of(reader: PipeReader) -> Grandchild {
        let mut reader = BufReader::new(reader);
        let group = group_on(&mut reader).expect("read the wrapper's first line");
        Grandchild::rest_of(group, reader)
    }

    /// The tree of group `group`, whose line was read off `reader`; reads the rest on another
    /// thread.
    fn rest_of(group: i32, mut reader: BufReader<PipeReader>) -> Grandchild {
        let (sender, eof) = mpsc::channel();
        thread::spawn(move || {
            let mut rest = Vec::new();
            drop(sender.send(reader.read_to_end(&mut rest).map(|_| rest)));
        });
        Grandchild { group, eof }
    }

    /// Waits for EOF on the pipe, which comes once the wrapper and the grandchild are gone, and
    /// returns what they printed after the group. A tree still holding the pipe after `SURVIVOR`
    /// fails the test (`fail`).
    pub(in crate::process) fn gone(self) -> Vec<u8> {
        match self.eof.recv_timeout(SURVIVOR) {
            Ok(rest) => rest.expect("read the pipe"),
            Err(_) => self.fail(&format!("it outlived its stop by {SURVIVOR:?}")),
        }
    }

    /// SIGKILL to the group, then fails the test with `why`, so a broken stop leaves no tree
    /// behind. Called only while the tree is known to hold the pipe, so a process is still in the
    /// group, and a group's id is not given to a new process while one is.
    pub(in crate::process) fn fail(self, why: &str) -> ! {
        let killed = killpg(Pid::from_raw(self.group), Signal::SIGKILL);
        panic!(
            "the tree of group {}: {why} (SIGKILL to it: {killed:?})",
            self.group
        );
    }
}

/// The group a wrapper printed on the next line of `reader`.
fn group_on(reader: &mut BufReader<PipeReader>) -> io::Result<i32> {
    let mut line = String::new();
    reader.read_line(&mut line)?;
    line.trim().parse().map_err(|error| {
        io::Error::other(format!(
            "the wrapper ended before it printed its group: {line:?} ({error})"
        ))
    })
}

/// Starts `sh -c <script>` with `limits`, its stdout the write end of a new pipe; returns the run
/// and the read end.
pub(in crate::process) fn start(script: &str, limits: Limits) -> (Running, PipeReader) {
    let (reader, writer) = running::pipe().expect("make a pipe");
    let mut command = Command::new("sh");
    command.args(["-c", script]).stdout(writer);
    let running = Running::start(command, "sh", Streams::Inherit, limits).expect("sh starts");
    (running, reader)
}

#[test]
fn a_timeout_stops_the_command_and_its_grandchild() {
    let (running, reader) = start(WRAPPER, Limits::of(SHORT_LIMIT));
    let grandchild = Grandchild::of(reader);
    let finish = running.finish(&mut |_, _| {});
    let rest = grandchild.gone();
    assert!(matches!(finish, Ok(Finish::TimedOut)), "{finish:?}");
    assert_eq!(rest, b"");
}

#[test]
fn a_timeout_sends_sigterm_before_sigkill() {
    let (running, reader) = start(REPORTS_SIGTERM, Limits::of(SHORT_LIMIT));
    let grandchild = Grandchild::of(reader);
    let finish = running.finish(&mut |_, _| {});
    let rest = grandchild.gone();
    assert!(matches!(finish, Ok(Finish::TimedOut)), "{finish:?}");
    assert_eq!(String::from_utf8_lossy(&rest), "term\n");
}

#[test]
fn a_drop_stops_the_command_and_its_grandchild() {
    let (running, reader) = start(WRAPPER, Limits::of(TEST_TIMEOUT));
    let grandchild = Grandchild::of(reader);
    drop(running);
    assert_eq!(grandchild.gone(), b"");
}

#[test]
fn a_drop_sends_sigterm_before_sigkill() {
    let (running, reader) = start(REPORTS_SIGTERM, Limits::of(TEST_TIMEOUT));
    let grandchild = Grandchild::of(reader);
    drop(running);
    assert_eq!(String::from_utf8_lossy(&grandchild.gone()), "term\n");
}

#[test]
fn a_panic_stops_the_command_and_its_grandchild() {
    let (running, reader) = start(WRAPPER, Limits::of(TEST_TIMEOUT));
    let grandchild = Grandchild::of(reader);
    let panicked = panic::catch_unwind(AssertUnwindSafe(move || {
        let _held = running;
        panic!("a panic while the command runs");
    }));
    assert!(panicked.is_err());
    assert_eq!(grandchild.gone(), b"");
}

#[test]
fn a_grandchild_left_running_is_stopped_when_the_command_exits() {
    let (running, reader) = start("sleep 300 & echo $$", Limits::of(TEST_TIMEOUT));
    let grandchild = Grandchild::of(reader);
    let finish = running.finish(&mut |_, _| {});
    let rest = grandchild.gone();
    assert!(
        matches!(&finish, Ok(Finish::Exited(status)) if status.success()),
        "{finish:?}"
    );
    assert_eq!(rest, b"");
}

/// Leaves one process in its group when it exits, a perl that on SIGTERM prints `term` and exits.
/// It prints the group's id once its handler is set, so the stop at the exit, which comes only
/// once `finish` runs, finds it set. One process, not a shell with a `sleep`: a SIGTERM that
/// lands while a shell forks its `sleep` can leave the `sleep` running.
const LEFTOVER_REPORTS_SIGTERM: &str = r#"perl -e '$SIG{TERM} = sub { print "term\n"; exit 0 };
$| = 1; print getpgrp(), "\n"; sleep 300' &"#;

/// Leaves one process in its group, a perl that ignores SIGTERM, and prints the group's id once
/// it does.
const LEFTOVER_IGNORES_SIGTERM: &str =
    r#"perl -e '$SIG{TERM} = "IGNORE"; $| = 1; print getpgrp(), "\n"; sleep 300' &"#;

#[test]
fn a_leftover_gets_sigterm_when_the_command_exits_and_the_run_ends_when_it_does() {
    let limits = Limits {
        grace: SURVIVOR,
        ..Limits::of(TEST_TIMEOUT)
    };
    let (running, reader) = start(LEFTOVER_REPORTS_SIGTERM, limits);
    let grandchild = Grandchild::of(reader);
    let (finish, secs) = timed(|| running.finish(&mut |_, _| {}));
    let rest = grandchild.gone();
    assert!(
        matches!(&finish, Ok(Finish::Exited(status)) if status.success()),
        "{finish:?}"
    );
    assert_eq!(String::from_utf8_lossy(&rest), "term\n");
    let grace = limits.grace.as_secs_f64();
    assert!(
        secs < grace,
        "ended after {secs} s, not before the {grace} s grace"
    );
}

#[test]
fn a_leftover_that_ignores_sigterm_gets_sigkill_after_the_grace_period() {
    let limits = Limits {
        grace: SHORT_WAIT,
        ..Limits::of(TEST_TIMEOUT)
    };
    let (running, reader) = start(LEFTOVER_IGNORES_SIGTERM, limits);
    let grandchild = Grandchild::of(reader);
    let (finish, secs) = timed(|| running.finish(&mut |_, _| {}));
    let rest = grandchild.gone();
    assert!(
        matches!(&finish, Ok(Finish::Exited(status)) if status.success()),
        "{finish:?}"
    );
    assert_eq!(rest, b"");
    let grace = limits.grace.as_secs_f64();
    assert!(
        secs >= grace,
        "stopped after {secs} s, before the {grace} s grace"
    );
}

#[test]
fn a_thread_that_cannot_start_stops_and_reaps_the_command() {
    let (reader, writer) = running::pipe().expect("make a pipe");
    let mut command = Command::new("sh");
    command.args(["-c", WRAPPER]).stdout(writer);
    let reader = Rc::new(RefCell::new(BufReader::new(reader)));
    let group = Rc::new(Cell::new(0));
    let (hook_reader, hook_group) = (Rc::clone(&reader), Rc::clone(&group));
    // The waiter's thread is the first a run starts; it fails once the wrapper is whole.
    running::BEFORE_THREAD.set(Some(Box::new(move || {
        hook_group.set(group_on(&mut hook_reader.borrow_mut())?);
        Err(io::Error::other("the test's thread could not start"))
    })));
    let started = Running::start(command, "sh", Streams::Inherit, Limits::of(TEST_TIMEOUT));
    running::BEFORE_THREAD.set(None);
    let Err(error) = started else {
        panic!("the run started without its waiter's thread");
    };
    assert_eq!(error.to_string(), "the test's thread could not start");
    let reader = Rc::into_inner(reader)
        .expect("the hook is gone")
        .into_inner();
    let grandchild = Grandchild::rest_of(group.get(), reader);
    // A zombie still answers signal 0; only a reaped pid gives ESRCH.
    let wrapper = kill(Pid::from_raw(grandchild.group()), None);
    assert_eq!(wrapper, Err(Errno::ESRCH), "the wrapper was not reaped");
    assert_eq!(grandchild.gone(), b"");
}

#[test]
fn a_tree_that_ignores_sigterm_gets_sigkill_after_the_grace_period() {
    let limits = Limits {
        grace: SHORT_WAIT,
        ..Limits::of(SHORT_LIMIT)
    };
    let (running, reader) = start(&format!("trap '' TERM; {WRAPPER}"), limits);
    let grandchild = Grandchild::of(reader);
    let (finish, secs) = timed(|| running.finish(&mut |_, _| {}));
    let rest = grandchild.gone();
    assert!(matches!(finish, Ok(Finish::TimedOut)), "{finish:?}");
    assert_eq!(rest, b"");
    let least = (limits.run + limits.grace).as_secs_f64();
    assert!(secs >= least, "stopped after {secs} s, before {least} s");
}

#[test]
fn a_command_with_its_own_grace_gets_sigkill_after_that_grace() {
    let script = format!("trap '' TERM; {WRAPPER}");
    let cmd = Cmd::new(Program::Named("sh"), ["-c", &script], SHORT_LIMIT).kill_grace(SHORT_WAIT);
    let limits = Limits::of_cmd(&cmd);
    assert_eq!(
        limits,
        Limits {
            run: SHORT_LIMIT,
            grace: SHORT_WAIT,
            drain: Duration::from_secs(1),
        }
    );
    let (running, reader) = start(&script, limits);
    let grandchild = Grandchild::of(reader);
    let (finish, secs) = timed(|| running.finish(&mut |_, _| {}));
    let rest = grandchild.gone();
    assert!(matches!(finish, Ok(Finish::TimedOut)), "{finish:?}");
    assert_eq!(rest, b"");
    let least = (limits.run + limits.grace).as_secs_f64();
    let default = (limits.run + running::KILL_GRACE).as_secs_f64();
    assert!(
        (least..default).contains(&secs),
        "stopped after {secs} s, not between {least} s and the {default} s of the default grace"
    );
}

#[test]
fn every_command_gets_five_seconds_of_grace_and_one_second_to_drain() {
    let cmd = Cmd::new(Program::Named("true"), Vec::<String>::new(), TEST_TIMEOUT);
    assert_eq!(Limits::of_cmd(&cmd), Limits::of(TEST_TIMEOUT));
    assert_eq!(
        Limits::of(TEST_TIMEOUT),
        Limits {
            run: TEST_TIMEOUT,
            grace: Duration::from_secs(5),
            drain: Duration::from_secs(1),
        }
    );
}

/// A perl script that forks a child, which leaves its process group (`setsid`) and writes to
/// stdout until the pipe's reader is gone (SIGPIPE, or the write fails). The parent exits once
/// the child has left the group: the child's `close $w` ends the parent's read.
const ESCAPES: &str = r#"pipe(my $r, my $w) or die "pipe: $!";
defined(my $pid = fork) or die "fork: $!";
if ($pid) { close $w; <$r>; exit 0 }
close $r; POSIX::setsid() or die "setsid: $!"; close $w;
my $y = "y" x 4096; 1 while defined syswrite(STDOUT, $y);"#;

#[test]
fn a_process_that_left_the_group_with_the_output_ends_the_run_after_the_drain() {
    let mut command = Command::new("perl");
    command.args(["-MPOSIX", "-e", ESCAPES]);
    let limits = Limits {
        drain: SHORT_WAIT,
        ..Limits::of(TEST_TIMEOUT)
    };
    let running = Running::start(command, "perl", Streams::Merged, limits).expect("perl starts");
    let (sender, finished) = mpsc::channel();
    // On another thread, so a run that waits on the pipe for ever fails the test instead.
    thread::spawn(move || drop(sender.send(timed(|| running.finish(&mut |_, _| {})))));
    let Ok((finish, secs)) = finished.recv_timeout(SURVIVOR) else {
        panic!("the run still waited for its output {SURVIVOR:?} after it started");
    };
    let error = finish.expect_err("a pipe was still open after the drain");
    assert_eq!(
        error.to_string(),
        "`perl` exited, but its output was still open 200 ms after its process group was \
         stopped: a process it started left the process group and holds it, and is left running"
    );
    assert!(secs >= limits.drain.as_secs_f64(), "ended after {secs} s");
}

#[test]
fn a_leftover_in_the_group_that_holds_the_output_is_stopped_when_the_command_exits() {
    let cmd = Cmd::new(
        Program::Named("sh"),
        ["-c", "sleep 300 & echo hi"],
        TEST_TIMEOUT,
    );
    let captured = capture(&cmd, Path::new("."), &mut |_| {}).expect("sh runs");
    assert!(captured.success());
    assert_eq!(captured.stdout, "hi\n");
}

#[test]
fn every_runner_gives_a_command_past_its_limit_as_a_status_not_an_error() {
    let cmd = Cmd::new(Program::Named("sleep"), ["300"], SHORT_LIMIT);
    let captured = capture(&cmd, Path::new("."), &mut |_| {}).expect("sleep starts");
    assert_eq!(captured.status, Status::TimedOut);
    assert!(!captured.success());
    let quiet = run_quiet(&cmd, Path::new(".")).expect("sleep starts");
    assert_eq!(quiet.status, Status::TimedOut);
    let inherited = run(&cmd, Path::new("."), Output::Inherit).expect("sleep starts");
    assert_eq!(inherited.status, Status::TimedOut);
}

#[test]
fn run_quiet_reports_a_command_that_exits_before_its_limit() {
    let cmd = Cmd::new(Program::Named("sh"), ["-c", "echo done"], TEST_TIMEOUT);
    let quiet = run_quiet(&cmd, Path::new(".")).expect("sh runs");
    assert!(quiet.success());
    assert_eq!(quiet.stdout, "done\n");
}
