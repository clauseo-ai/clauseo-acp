//! The process door when xtask itself dies. Each test starts `xtask process-test` (hidden from
//! `--help`), which runs a wrapper through the door; the wrapper starts a grandchild, prints its
//! pid and waits. Then the test kills xtask: with SIGKILL, which no handler sees, or with SIGINT to
//! xtask's process group, as a terminal sends Ctrl-C to its foreground job. Since the door starts
//! every command in a group of its own, only xtask gets the SIGINT; it dies of it, and the leader
//! must then stop the wrapper's group.
//!
//! The grandchild holds a pipe the test reads: `sh` makes fd 3 a copy of it without close-on-exec,
//! so xtask, the wrapper and the grandchild inherit it (and the leader, which exits once the groups
//! it stops are empty). The test sees EOF only once all of them are gone, and never sleeps. A
//! survivor fails the test and is not killed: by then its pid may be another process's. It ends
//! when its `sleep` does, and a stuck xtask at its `process-test` limit.
#![cfg(unix)]

// In a `#[cfg(test)]` module, so the helpers may panic as the tests do.
#[cfg(test)]
mod xtask_dies {
    use nix::sys::signal::{Signal, killpg};
    use nix::unistd::Pid;
    use std::io::{self, BufRead, BufReader, PipeReader, Read};
    use std::os::unix::process::{CommandExt, ExitStatusExt};
    use std::process::{Child, Command, ExitStatus, Stdio};
    use std::sync::mpsc;
    use std::sync::{Mutex, PoisonError};
    use std::thread;
    use std::time::Duration;

    /// Starts a grandchild that holds fd 3, prints the grandchild's pid on fd 3, and waits for it.
    const WRAPPER: &str = "sleep 300 & echo $! >&3; wait";

    /// `WRAPPER`, but on SIGTERM it prints `term` on fd 3 and exits, so it prints that only when
    /// the leader sends SIGTERM before SIGKILL.
    const REPORTS_SIGTERM: &str =
        "trap 'echo term >&3; exit 0' TERM; sleep 300 & echo $! >&3; wait";

    /// For the `restarted-leader` mode: `WRAPPER` on its first run; on its second, which runs
    /// once a new leader has started, it prints `again` on fd 3.
    const RUNS_AGAIN: &str =
        "if [ \"$1\" = again ]; then echo again >&3; else sleep 300 & echo $! >&3; wait; fi";

    /// How long the test waits for xtask to exit, and then for EOF, before it fails: far longer
    /// than the leader's grace period (5 s) and its check of the groups.
    const SURVIVOR: Duration = Duration::from_secs(25);

    /// Held while a pipe is made and xtask starts, so no xtask another test starts inherits this
    /// one's pipe (on macOS a pipe is marked close-on-exec only after it is made).
    static STARTING: Mutex<()> = Mutex::new(());

    /// Starts `xtask process-test <output> <script>` in a process group of its own, as a shell
    /// starts a job, with fd 3 the write end of a new pipe. Returns xtask and the read end.
    #[expect(
        clippy::disallowed_methods,
        reason = "the test starts the xtask binary itself, to kill it"
    )]
    fn start_xtask(output: &str, script: &str) -> (Child, PipeReader) {
        let _starting = STARTING.lock().unwrap_or_else(PoisonError::into_inner);
        let (reader, writer) = io::pipe().expect("make a pipe");
        let xtask = Command::new("sh")
            .args([
                "-c",
                r#"exec "$0" process-test "$1" "$2" 3>&1 >/dev/null"#,
                env!("CARGO_BIN_EXE_xtask"),
                output,
                script,
            ])
            .stdin(Stdio::null())
            .stdout(writer)
            .process_group(0)
            .spawn()
            .expect("xtask starts");
        (xtask, reader)
    }

    /// The pid of the grandchild, from the first line on `reader`, and the reader.
    fn grandchild_of(reader: PipeReader) -> (i32, BufReader<PipeReader>) {
        let mut reader = BufReader::new(reader);
        let pid = pid_of(&line_of(&mut reader));
        (pid, reader)
    }

    /// The next line on `reader`, without its newline.
    fn line_of(reader: &mut BufReader<PipeReader>) -> String {
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .expect("read the wrapper's line");
        line.trim().to_owned()
    }

    /// The pid on `line`.
    fn pid_of(line: &str) -> i32 {
        line.parse()
            .unwrap_or_else(|_| panic!("the wrapper printed no pid: {line:?}"))
    }

    /// Waits for `xtask` to exit, and fails when it is still running after `SURVIVOR`.
    fn exit_of(mut xtask: Child) -> ExitStatus {
        let pid = xtask.id();
        let (sender, exited) = mpsc::channel();
        thread::spawn(move || drop(sender.send(xtask.wait())));
        let Ok(status) = exited.recv_timeout(SURVIVOR) else {
            panic!("xtask {pid} was still running {SURVIVOR:?} after the signal");
        };
        status.expect("wait for xtask")
    }

    /// Waits for EOF on `reader`, which comes once the wrapper and the grandchild are gone, and
    /// returns what was printed after the lines read so far. A grandchild still holding the pipe
    /// after `SURVIVOR` fails the test.
    fn gone(mut reader: BufReader<PipeReader>, grandchild: i32) -> String {
        let (sender, eof) = mpsc::channel();
        thread::spawn(move || {
            let mut rest = Vec::new();
            drop(sender.send(reader.read_to_end(&mut rest).map(|_| rest)));
        });
        let Ok(rest) = eof.recv_timeout(SURVIVOR) else {
            panic!("grandchild {grandchild} outlived xtask by {SURVIVOR:?}");
        };
        String::from_utf8_lossy(&rest.expect("read the pipe")).into_owned()
    }

    /// Sends SIGINT to the process group `xtask` leads, as a terminal does on Ctrl-C.
    fn ctrl_c(xtask: &Child) {
        let group = Pid::from_raw(i32::try_from(xtask.id()).expect("a pid"));
        killpg(group, Signal::SIGINT).expect("send SIGINT to xtask's group");
    }

    #[test]
    fn process_leader_stops_the_tree_when_xtask_gets_sigkill() {
        let (mut xtask, reader) = start_xtask("capture", WRAPPER);
        let (grandchild, reader) = grandchild_of(reader);
        xtask.kill().expect("send SIGKILL to xtask");
        assert_eq!(exit_of(xtask).signal(), Some(Signal::SIGKILL as i32));
        assert_eq!(gone(reader, grandchild), "");
    }

    #[test]
    fn process_ctrl_c_stops_an_inherited_child_and_its_grandchild() {
        let (xtask, reader) = start_xtask("inherit", WRAPPER);
        let (grandchild, reader) = grandchild_of(reader);
        ctrl_c(&xtask);
        assert_eq!(exit_of(xtask).signal(), Some(Signal::SIGINT as i32));
        assert_eq!(gone(reader, grandchild), "");
    }

    #[test]
    fn process_ctrl_c_stops_a_captured_child_and_its_grandchild() {
        let (xtask, reader) = start_xtask("capture", WRAPPER);
        let (grandchild, reader) = grandchild_of(reader);
        ctrl_c(&xtask);
        assert_eq!(exit_of(xtask).signal(), Some(Signal::SIGINT as i32));
        assert_eq!(gone(reader, grandchild), "");
    }

    #[test]
    fn process_leader_sends_sigterm_before_sigkill() {
        let (mut xtask, reader) = start_xtask("capture", REPORTS_SIGTERM);
        let (grandchild, reader) = grandchild_of(reader);
        xtask.kill().expect("send SIGKILL to xtask");
        assert_eq!(exit_of(xtask).signal(), Some(Signal::SIGKILL as i32));
        assert_eq!(gone(reader, grandchild), "term\n");
    }

    #[test]
    fn process_a_new_leader_stops_the_tree_its_dead_predecessor_was_told_about() {
        let (mut xtask, reader) = start_xtask("restarted-leader", RUNS_AGAIN);
        let mut reader = BufReader::new(reader);
        // The first run prints its grandchild's pid and the second `again`, in either order.
        let (first, second) = (line_of(&mut reader), line_of(&mut reader));
        let pid = if first == "again" {
            second
        } else {
            assert_eq!(second, "again");
            first
        };
        let grandchild = pid_of(&pid);
        xtask.kill().expect("send SIGKILL to xtask");
        assert_eq!(exit_of(xtask).signal(), Some(Signal::SIGKILL as i32));
        assert_eq!(gone(reader, grandchild), "");
    }
}
