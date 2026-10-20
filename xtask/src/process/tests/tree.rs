//! Tests of how a command's tree is stopped: at its limit, when its run is dropped or panics,
//! after it exits, and with SIGKILL when it ignores SIGTERM. Most run a wrapper that starts a
//! grandchild and waits, as `ui/app/scripts/buildtime-proc.test.mjs` does. The grandchild holds
//! the write end of a pipe the test reads, so the test sees EOF only once the wrapper and the
//! grandchild are both gone. Nothing sleeps: the test blocks on that pipe or on the run.
//!
//! A run's limit counts from the start of `Running::finish`, and these tests read the
//! grandchild's pid before they call it, so a stop at the limit always meets the whole tree,
//! however loaded the machine. That is why their limits can be short.

use super::super::running::{self, Finish, Limits, Running, Streams};
use super::*;
use crate::clock::timed;
use std::io::{BufRead, BufReader, PipeReader, Read};
use std::panic::{self, AssertUnwindSafe};
use std::process::Command;
use std::sync::mpsc::{self, Receiver};
use std::thread;

/// Starts a grandchild that holds stdout, prints the grandchild's pid, and waits for it.
const WRAPPER: &str = "sleep 300 & echo $!; wait";

/// `WRAPPER`, but on SIGTERM it prints `term` and exits. So with SIGTERM first its run ends long
/// before `KILL_GRACE`, and with SIGKILL alone it prints nothing.
const REPORTS_SIGTERM: &str = "trap 'echo term; exit 0' TERM; sleep 300 & echo $!; wait";

/// The limit of a run the tests let time out. The tree is whole before it starts (see the top of
/// this file), so it only has to be short.
const SHORT_LIMIT: Duration = Duration::from_millis(50);

/// The grace of the test that waits it out, and the drain of the test that waits that out.
const SHORT_WAIT: Duration = Duration::from_millis(200);

/// How long a test waits for EOF, or for a run to end, before it fails: far longer than the
/// `KILL_GRACE` and drain a stop takes.
const SURVIVOR: Duration = Duration::from_secs(20);

/// A grandchild whose pid the wrapper printed, and the EOF of the pipe it holds.
struct Grandchild {
    pid: i32,
    eof: Receiver<io::Result<Vec<u8>>>,
}

impl Grandchild {
    /// Reads the pid the wrapper printed on `reader`'s first line, then reads the rest on another
    /// thread.
    fn of(reader: PipeReader) -> Grandchild {
        let mut reader = BufReader::new(reader);
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .expect("read the wrapper's first line");
        let pid = line.trim().parse().unwrap_or_else(|_| {
            panic!("the wrapper ended before it printed its grandchild's pid: {line:?}")
        });
        let (sender, eof) = mpsc::channel();
        thread::spawn(move || {
            let mut rest = Vec::new();
            drop(sender.send(reader.read_to_end(&mut rest).map(|_| rest)));
        });
        Grandchild { pid, eof }
    }

    /// Waits for EOF on the pipe, which comes once the wrapper and the grandchild are gone, and
    /// returns what they printed after the pid. A grandchild still holding the pipe after
    /// `SURVIVOR` fails the test. It is not killed: its pid may belong to another process by
    /// then. It ends when its `sleep` does.
    fn gone(self) -> Vec<u8> {
        let Ok(rest) = self.eof.recv_timeout(SURVIVOR) else {
            panic!("grandchild {} outlived its stop by {SURVIVOR:?}", self.pid);
        };
        rest.expect("read the pipe")
    }
}

/// Starts `sh -c <script>` with `limits`, its stdout the write end of a new pipe; returns the run
/// and the read end.
fn start(script: &str, limits: Limits) -> (Running, PipeReader) {
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
    assert!(matches!(finish, Ok(Finish::TimedOut)), "{finish:?}");
    assert_eq!(grandchild.gone(), b"");
}

#[test]
fn a_timeout_sends_sigterm_before_sigkill() {
    let (running, reader) = start(REPORTS_SIGTERM, Limits::of(SHORT_LIMIT));
    let grandchild = Grandchild::of(reader);
    let finish = running.finish(&mut |_, _| {});
    assert!(matches!(finish, Ok(Finish::TimedOut)), "{finish:?}");
    assert_eq!(String::from_utf8_lossy(&grandchild.gone()), "term\n");
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
    let (running, reader) = start("sleep 300 & echo $!", Limits::of(TEST_TIMEOUT));
    let grandchild = Grandchild::of(reader);
    let finish = running.finish(&mut |_, _| {});
    assert!(
        matches!(&finish, Ok(Finish::Exited(status)) if status.success()),
        "{finish:?}"
    );
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
    assert!(matches!(finish, Ok(Finish::TimedOut)), "{finish:?}");
    assert_eq!(grandchild.gone(), b"");
    let least = (limits.run + limits.grace).as_secs_f64();
    assert!(secs >= least, "stopped after {secs} s, before {least} s");
}

#[test]
fn every_command_gets_five_seconds_of_grace_and_one_second_to_drain() {
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
fn a_leftover_in_the_group_that_holds_the_output_is_killed_when_the_command_exits() {
    let cmd = Cmd::new(
        Program::Named("sh"),
        ["-c", "sleep 300 & echo hi"],
        TEST_TIMEOUT,
    );
    let captured = capture(&cmd, Path::new("."), &mut |_| {}).expect("sh runs");
    assert!(captured.success);
    assert_eq!(captured.output, "hi\n");
}

#[test]
fn capture_says_a_command_past_its_limit_was_stopped() {
    let cmd = Cmd::new(Program::Named("sleep"), ["300"], SHORT_LIMIT);
    let error = capture(&cmd, Path::new("."), &mut |_| {}).expect_err("sleep runs past its limit");
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    assert_eq!(
        error.to_string(),
        "`sleep 300` did not finish within 50 ms, so it was stopped"
    );
}

#[test]
fn run_quiet_reports_a_command_that_exits_before_its_limit() {
    let cmd = Cmd::new(Program::Named("sh"), ["-c", "echo done"], TEST_TIMEOUT);
    let quiet = run_quiet(&cmd, Path::new(".")).expect("sh runs");
    assert!(quiet.success);
    assert_eq!(quiet.stdout, "done\n");
}
