//! The process door when xtask itself dies. Each test starts `xtask process-test` (hidden from
//! `--help`), which runs a wrapper through the door; the wrapper starts a grandchild, prints its
//! own pid (its group's id) and waits. Then the test kills xtask: with SIGKILL, which no handler
//! sees, or with SIGINT to xtask's process group, as a terminal sends Ctrl-C to its foreground job.
//! Since the door starts every command in a group of its own, only xtask gets the SIGINT; it dies
//! of it, and the leader must then stop the wrapper's group.
//!
//! The grandchild holds a pipe the test reads: `sh` makes fd 3 a copy of it without close-on-exec,
//! so xtask, the wrapper and the grandchild inherit it (and the leader, which exits once the groups
//! it stops are empty). The test sees EOF only once all of them are gone, and never sleeps. A
//! survivor's group gets SIGKILL before the test fails; a stuck xtask ends at its `process-test`
//! limit.
//!
//! One test lets the script exit instead: it prints what an `Output::Inherit` command sees of
//! git's prompt variables, which the door sets because such a command cannot read the terminal.
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

    /// Starts a grandchild that holds fd 3, prints its own pid (its group's id) on fd 3, and waits.
    const WRAPPER: &str = "sleep 300 & echo $$ >&3; wait";

    /// `WRAPPER`, but on SIGTERM it prints `term` on fd 3 and exits, so it prints that only when
    /// the leader sends SIGTERM before SIGKILL.
    const REPORTS_SIGTERM: &str =
        "trap 'echo term >&3; exit 0' TERM; sleep 300 & echo $$ >&3; wait";

    /// For the `restarted-leader` mode: `WRAPPER` on its first run; on its second, which runs
    /// once a new leader has started, it prints `again` on fd 3.
    const RUNS_AGAIN: &str =
        "if [ \"$1\" = again ]; then echo again >&3; else sleep 300 & echo $$ >&3; wait; fi";

    /// How long the test waits for xtask to exit, and then for EOF, before it fails: far longer
    /// than the leader's grace period (5 s) and its check of the groups.
    const SURVIVOR: Duration = Duration::from_secs(25);

    /// Held while a pipe is made and xtask starts, so no xtask another test starts inherits this
    /// one's pipe (on macOS a pipe is marked close-on-exec only after it is made).
    static STARTING: Mutex<()> = Mutex::new(());

    /// Starts `xtask process-test <output> <script>` in a process group of its own, as a shell
    /// starts a job, with fd 3 the write end of a new pipe, and `env` given its command to change
    /// its environment. Returns xtask and the read end.
    #[expect(
        clippy::disallowed_methods,
        reason = "the test starts the xtask binary itself, to kill it"
    )]
    fn start_xtask(
        output: &str,
        script: &str,
        env: &mut dyn FnMut(&mut Command),
    ) -> (Child, PipeReader) {
        let _starting = STARTING.lock().unwrap_or_else(PoisonError::into_inner);
        let (reader, writer) = io::pipe().expect("make a pipe");
        let mut command = Command::new("sh");
        env(&mut command);
        let xtask = command
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

    /// The wrapper's group, from the first line on `reader`, and the reader.
    fn group_of(reader: PipeReader) -> (i32, BufReader<PipeReader>) {
        let ([line], reader) = lines_of(BufReader::new(reader));
        (pid_of(&line), reader)
    }

    /// The next `N` lines on `reader`, without their newlines, and the reader. Fails when they
    /// have not all come after `SURVIVOR`, rather than wait for ever for a line that a broken
    /// door never has printed.
    fn lines_of<const N: usize>(
        mut reader: BufReader<PipeReader>,
    ) -> ([String; N], BufReader<PipeReader>) {
        let (sender, read) = mpsc::channel();
        thread::spawn(move || {
            let lines = [(); N].map(|()| {
                let mut line = String::new();
                reader
                    .read_line(&mut line)
                    .expect("read the wrapper's line");
                line.trim().to_owned()
            });
            drop(sender.send((lines, reader)));
        });
        read.recv_timeout(SURVIVOR)
            .unwrap_or_else(|_| panic!("the wrapper printed no {N} lines within {SURVIVOR:?}"))
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
    /// returns what was printed after the lines read so far. A tree still holding the pipe after
    /// `SURVIVOR` gets SIGKILL to `group`, and the test fails. Safe: a process still holds the
    /// pipe, so it is still in the group, and a group's id is not given out while one is.
    fn gone(mut reader: BufReader<PipeReader>, group: i32) -> String {
        let (sender, eof) = mpsc::channel();
        thread::spawn(move || {
            let mut rest = Vec::new();
            drop(sender.send(reader.read_to_end(&mut rest).map(|_| rest)));
        });
        let Ok(rest) = eof.recv_timeout(SURVIVOR) else {
            let killed = killpg(Pid::from_raw(group), Signal::SIGKILL);
            panic!("group {group} outlived xtask by {SURVIVOR:?} (SIGKILL to it: {killed:?})");
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
        let (mut xtask, reader) = start_xtask("capture", WRAPPER, &mut |_| {});
        let (group, reader) = group_of(reader);
        xtask.kill().expect("send SIGKILL to xtask");
        assert_eq!(exit_of(xtask).signal(), Some(Signal::SIGKILL as i32));
        assert_eq!(gone(reader, group), "");
    }

    #[test]
    fn process_ctrl_c_stops_an_inherited_child_and_its_grandchild() {
        let (xtask, reader) = start_xtask("inherit", WRAPPER, &mut |_| {});
        let (group, reader) = group_of(reader);
        ctrl_c(&xtask);
        assert_eq!(exit_of(xtask).signal(), Some(Signal::SIGINT as i32));
        assert_eq!(gone(reader, group), "");
    }

    #[test]
    fn process_ctrl_c_stops_a_captured_child_and_its_grandchild() {
        let (xtask, reader) = start_xtask("capture", WRAPPER, &mut |_| {});
        let (group, reader) = group_of(reader);
        ctrl_c(&xtask);
        assert_eq!(exit_of(xtask).signal(), Some(Signal::SIGINT as i32));
        assert_eq!(gone(reader, group), "");
    }

    #[test]
    fn process_leader_sends_sigterm_before_sigkill() {
        let (mut xtask, reader) = start_xtask("capture", REPORTS_SIGTERM, &mut |_| {});
        let (group, reader) = group_of(reader);
        xtask.kill().expect("send SIGKILL to xtask");
        assert_eq!(exit_of(xtask).signal(), Some(Signal::SIGKILL as i32));
        assert_eq!(gone(reader, group), "term\n");
    }

    /// Prints on fd 3 what an `Output::Inherit` command sees of git's prompt variables.
    const PROMPT_ENV: &str = r#"echo "$GIT_TERMINAL_PROMPT|$GIT_SSH_COMMAND" >&3"#;

    /// What `xtask process-test <output> PROMPT_ENV` prints, with `GIT_SSH_COMMAND` set to
    /// `ssh` in xtask's environment, or removed from it. Fails when xtask does not exit 0.
    fn prompt_env(output: &str, ssh: Option<&str>) -> String {
        let (xtask, reader) = start_xtask(output, PROMPT_ENV, &mut |command| {
            command.env_remove("GIT_TERMINAL_PROMPT");
            match ssh {
                Some(ssh) => command.env("GIT_SSH_COMMAND", ssh),
                None => command.env_remove("GIT_SSH_COMMAND"),
            };
        });
        let ([line], _) = lines_of(BufReader::new(reader));
        assert!(exit_of(xtask).success(), "the script exits 0");
        line
    }

    #[test]
    fn process_an_inherited_child_gets_git_and_ssh_prompts_turned_off() {
        assert_eq!(prompt_env("inherit", None), "0|ssh -o BatchMode=yes");
        assert_eq!(prompt_env("inherit", Some("ssh -i key")), "0|ssh -i key");
        assert_eq!(
            prompt_env("capture", None),
            "|",
            "a captured child cannot prompt"
        );
    }

    #[test]
    fn process_a_new_leader_stops_the_tree_its_dead_predecessor_was_told_about() {
        let (mut xtask, reader) = start_xtask("restarted-leader", RUNS_AGAIN, &mut |_| {});
        // The first run prints its group and the second `again`, in either order.
        let ([first, second], reader) = lines_of(BufReader::new(reader));
        let pid = if first == "again" {
            second
        } else {
            assert_eq!(second, "again");
            first
        };
        let group = pid_of(&pid);
        xtask.kill().expect("send SIGKILL to xtask");
        assert_eq!(exit_of(xtask).signal(), Some(Signal::SIGKILL as i32));
        assert_eq!(gone(reader, group), "");
    }
}
