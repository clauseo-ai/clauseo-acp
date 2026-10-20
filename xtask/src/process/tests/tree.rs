//! Tests of how a command's tree is stopped: at its limit, when its run is dropped or panics,
//! after it exits, and with SIGKILL when it ignores SIGTERM. Each runs a wrapper that starts a
//! grandchild and waits, as `ui/app/scripts/buildtime-proc.test.mjs` does. The grandchild holds
//! the write end of a pipe the test reads, so the test sees EOF only once the wrapper and the
//! grandchild are both gone. Nothing sleeps: the test blocks on that pipe.

use super::super::running::{Finish, KILL_GRACE, Running, Streams};
use super::*;
use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;
use std::io::{BufRead, BufReader, PipeReader, PipeWriter, Read};

use std::panic::{self, AssertUnwindSafe};
use std::process::Command;
use std::sync::mpsc::{self, Receiver};
use std::thread;

/// Starts a grandchild that holds stdout, prints the grandchild's pid, and waits for it.
const WRAPPER: &str = "sleep 300 & echo $!; wait";

/// The limit of a run the tests let time out. Long enough that the wrapper has printed its
/// grandchild's pid before it, short enough to keep the tests quick.
const SHORT_LIMIT: Duration = Duration::from_secs(2);

/// How long a test waits for EOF before it calls the grandchild a survivor, kills it and fails:
/// far longer than the `KILL_GRACE` and drain a stop takes.
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
            panic!("the wrapper ended before it printed its grandchild's pid, so its run was stopped too soon: {line:?}")
        });
        let (sender, eof) = mpsc::channel();
        thread::spawn(move || {
            let mut rest = Vec::new();
            drop(sender.send(reader.read_to_end(&mut rest).map(|_| rest)));
        });
        Grandchild { pid, eof }
    }

    /// Waits for EOF on the pipe: the wrapper and the grandchild are gone. A grandchild still
    /// holding it after `SURVIVOR` is killed by its pid, and the test fails.
    fn assert_gone(self) {
        let Ok(rest) = self.eof.recv_timeout(SURVIVOR) else {
            if let Err(errno) = kill(Pid::from_raw(self.pid), Signal::SIGKILL) {
                eprintln!("could not kill grandchild {}: {errno}", self.pid);
            }
            panic!("grandchild {} outlived its stop by {SURVIVOR:?}", self.pid);
        };
        assert_eq!(rest.expect("read the pipe"), b"");
    }
}

/// `sh -c <script>` with stdout the write end of a new pipe, and the read end.
fn wrapper(script: &str) -> (Command, PipeReader) {
    let (reader, writer): (PipeReader, PipeWriter) = io::pipe().expect("make a pipe");
    let mut command = Command::new("sh");
    command.args(["-c", script]).stdout(writer);
    (command, reader)
}

/// Starts `script` with `limit`, its stdout the test's pipe.
fn start(script: &str, limit: Duration) -> (Running, PipeReader) {
    let (command, reader) = wrapper(script);
    let running = Running::start(command, "sh", Streams::Inherit, limit).expect("sh starts");
    (running, reader)
}

#[test]
fn a_timeout_stops_the_command_and_its_grandchild() {
    let (running, reader) = start(WRAPPER, SHORT_LIMIT);
    let finished = thread::spawn(move || running.finish(&mut |_, _| {}));
    Grandchild::of(reader).assert_gone();
    let finish = finished.join().expect("the run does not panic");
    assert!(matches!(finish, Ok(Finish::TimedOut)), "{finish:?}");
}

#[test]
fn a_drop_stops_the_command_and_its_grandchild() {
    let (running, reader) = start(WRAPPER, TEST_TIMEOUT);
    let grandchild = Grandchild::of(reader);
    drop(running);
    grandchild.assert_gone();
}

#[test]
fn a_panic_stops_the_command_and_its_grandchild() {
    let (running, reader) = start(WRAPPER, TEST_TIMEOUT);
    let grandchild = Grandchild::of(reader);
    let panicked = panic::catch_unwind(AssertUnwindSafe(move || {
        let _held = running;
        panic!("a panic while the command runs");
    }));
    assert!(panicked.is_err());
    grandchild.assert_gone();
}

#[test]
fn a_grandchild_left_running_is_stopped_when_the_command_exits() {
    let (running, reader) = start("sleep 300 & echo $!", TEST_TIMEOUT);
    let finished = thread::spawn(move || running.finish(&mut |_, _| {}));
    Grandchild::of(reader).assert_gone();
    let finish = finished.join().expect("the run does not panic");
    assert!(
        matches!(&finish, Ok(Finish::Exited(status)) if status.success()),
        "{finish:?}"
    );
}

#[test]
fn a_tree_that_ignores_sigterm_gets_sigkill_after_the_grace_period() {
    let (running, reader) = start(&format!("trap '' TERM; {WRAPPER}"), SHORT_LIMIT);
    let finished = thread::spawn(move || crate::clock::timed(|| running.finish(&mut |_, _| {})));
    Grandchild::of(reader).assert_gone();
    let (finish, secs) = finished.join().expect("the run does not panic");
    assert!(matches!(finish, Ok(Finish::TimedOut)), "{finish:?}");
    let least = (SHORT_LIMIT + KILL_GRACE).as_secs_f64();
    assert!(secs >= least, "stopped after {secs} s, before {least} s");
}

#[test]
fn capture_says_a_command_past_its_limit_was_stopped() {
    let cmd = Cmd::new(Program::Named("sh"), ["-c", WRAPPER], SHORT_LIMIT);
    let mut seen = Vec::new();
    let captured = capture(&cmd, Path::new("."), &mut |chunk| {
        seen.extend_from_slice(chunk);
    });
    let error = captured.err().expect("sh runs past its limit");
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    assert_eq!(
        error.to_string(),
        "`sh -c sleep 300 & echo $!; wait` did not finish within 2 s, so it was stopped"
    );
    // The wrapper printed its grandchild's pid before the limit, so the stop met a tree of two.
    let pid: i32 = String::from_utf8_lossy(&seen)
        .trim()
        .parse()
        .expect("a pid");
    assert!(pid > 0);
}

#[test]
fn run_quiet_reports_a_command_that_exits_before_its_limit() {
    let cmd = Cmd::new(Program::Named("sh"), ["-c", "echo done"], TEST_TIMEOUT);
    let quiet = run_quiet(&cmd, Path::new(".")).expect("sh runs");
    assert!(quiet.success);
    assert_eq!(quiet.stdout, "done\n");
}
