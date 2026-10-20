//! The process door when xtask itself dies. Each test starts `xtask process-test` (hidden from
//! `--help`), which runs a wrapper through the door; the wrapper starts a grandchild, prints its
//! pid and waits. Then the test kills xtask: with SIGKILL, which no handler sees, or with SIGINT to
//! xtask's process group, as a terminal sends Ctrl-C to its foreground job. Since the door starts
//! every command in a group of its own, only xtask gets the SIGINT; it dies of it, and the leader
//! must then stop the wrapper's group.
//!
//! The grandchild holds a pipe the test reads: `sh` makes fd 3 a copy of it without close-on-exec,
//! so xtask, the wrapper and the grandchild inherit it (and the leader, which exits once the groups
//! it stops are empty). The test sees EOF only once all of them are gone, and never sleeps.
#![cfg(unix)]

// In a `#[cfg(test)]` module, so the helpers may panic as the tests do.
#[cfg(test)]
mod xtask_dies {
    use nix::sys::signal::{Signal, kill, killpg};
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

    /// How long the test waits for xtask to exit, and then for EOF, before it fails: far longer
    /// than the leader's grace period (5 s) and its check of the groups.
    const SURVIVOR: Duration = Duration::from_secs(25);

    /// Held while a pipe is made and xtask starts, so no xtask another test starts inherits this
    /// one's pipe (on macOS a pipe is marked close-on-exec only after it is made).
    static STARTING: Mutex<()> = Mutex::new(());

    /// Starts `xtask process-test <output> <WRAPPER>` in a process group of its own, as a shell
    /// starts a job, with fd 3 the write end of a new pipe. Returns xtask and the read end.
    #[expect(
        clippy::disallowed_methods,
        reason = "the test starts the xtask binary itself, to kill it"
    )]
    fn start_xtask(output: &str) -> (Child, PipeReader) {
        let _starting = STARTING.lock().unwrap_or_else(PoisonError::into_inner);
        let (reader, writer) = io::pipe().expect("make a pipe");
        let xtask = Command::new("sh")
            .args([
                "-c",
                r#"exec "$0" process-test "$1" "$2" 3>&1 >/dev/null"#,
                env!("CARGO_BIN_EXE_xtask"),
                output,
                WRAPPER,
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
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .expect("read the wrapper's line");
        let pid = line
            .trim()
            .parse()
            .unwrap_or_else(|_| panic!("the wrapper printed no pid: {line:?}"));
        (pid, reader)
    }

    /// Waits for `xtask` to exit. Kills it and fails when it is still running after `SURVIVOR`.
    fn exit_of(mut xtask: Child) -> ExitStatus {
        let pid = xtask.id();
        let (sender, exited) = mpsc::channel();
        thread::spawn(move || drop(sender.send(xtask.wait())));
        let Ok(status) = exited.recv_timeout(SURVIVOR) else {
            let pid = Pid::from_raw(i32::try_from(pid).expect("a pid"));
            if let Err(errno) = kill(pid, Signal::SIGKILL) {
                eprintln!("could not kill xtask {pid}: {errno}");
            }
            panic!("xtask {pid} was still running {SURVIVOR:?} after the signal");
        };
        status.expect("wait for xtask")
    }

    /// Waits for EOF on `reader`: the wrapper and the grandchild are gone. A grandchild still
    /// holding it after `SURVIVOR` is killed by its pid, and the test fails.
    fn assert_gone(mut reader: BufReader<PipeReader>, grandchild: i32) {
        let (sender, eof) = mpsc::channel();
        thread::spawn(move || {
            let mut rest = Vec::new();
            drop(sender.send(reader.read_to_end(&mut rest).map(|_| rest)));
        });
        let Ok(rest) = eof.recv_timeout(SURVIVOR) else {
            if let Err(errno) = kill(Pid::from_raw(grandchild), Signal::SIGKILL) {
                eprintln!("could not kill grandchild {grandchild}: {errno}");
            }
            panic!("grandchild {grandchild} outlived xtask by {SURVIVOR:?}");
        };
        assert_eq!(rest.expect("read the pipe"), b"");
    }

    /// Sends SIGINT to the process group `xtask` leads, as a terminal does on Ctrl-C.
    fn ctrl_c(xtask: &Child) {
        let group = Pid::from_raw(i32::try_from(xtask.id()).expect("a pid"));
        killpg(group, Signal::SIGINT).expect("send SIGINT to xtask's group");
    }

    #[test]
    fn process_leader_stops_the_tree_when_xtask_gets_sigkill() {
        let (mut xtask, reader) = start_xtask("capture");
        let (grandchild, reader) = grandchild_of(reader);
        xtask.kill().expect("send SIGKILL to xtask");
        assert_eq!(exit_of(xtask).signal(), Some(Signal::SIGKILL as i32));
        assert_gone(reader, grandchild);
    }

    #[test]
    fn process_ctrl_c_stops_an_inherited_child_and_its_grandchild() {
        let (xtask, reader) = start_xtask("inherit");
        let (grandchild, reader) = grandchild_of(reader);
        ctrl_c(&xtask);
        assert_eq!(exit_of(xtask).signal(), Some(Signal::SIGINT as i32));
        assert_gone(reader, grandchild);
    }

    #[test]
    fn process_ctrl_c_stops_a_captured_child_and_its_grandchild() {
        let (xtask, reader) = start_xtask("capture");
        let (grandchild, reader) = grandchild_of(reader);
        ctrl_c(&xtask);
        assert_eq!(exit_of(xtask).signal(), Some(Signal::SIGINT as i32));
        assert_gone(reader, grandchild);
    }
}
