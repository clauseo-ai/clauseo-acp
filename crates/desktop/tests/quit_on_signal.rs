//! The quit on a signal (Unix, `src/quit_signals.rs`), each case in a process of its own: this
//! test binary run again through `clauseo_process::spawn_in_tree` to run one `role_*` function,
//! with [`ROLE`] naming it. The role installs the app's handlers (`QuitSignals::listen`) on
//! Tauri's runtime, so they never reach another test, and the test signals it from outside, as a
//! user, a terminal or launchd would.
//!
//! - `sigterm_stops_the_agent_and_its_child`: the role starts a real `clauseo_host` with
//!   `fake_agent` as its agent (`prompt-child-in-group`), imports a folder as a matter, prompts
//!   once, and the agent starts a child that stays in its process group and ignores stdin. On
//!   SIGTERM the handlers' first signal stops the host, as the app's `AppHandle::exit(0)` does
//!   through `stop_host_then_exit`; the role exits 0 within [`EXIT_WITHIN`], and neither the
//!   agent nor its child runs afterwards. Without the handlers SIGTERM ends the role at once, the
//!   agent exits on its stdin's end, and its child runs on. On Linux the agent and its child
//!   start with SIGTERM, SIGINT and SIGHUP neither blocked, ignored nor caught.
//! - `the_agent_dies_at_the_ladder_s_sigterm_under_the_app_s_handlers`: with the handlers in,
//!   `fake_agent --hold` (which ignores its stdin's end) started through `spawn_in_tree` ends at
//!   the ladder's SIGTERM step (`ShutdownOutcome::Terminated`), before the SIGKILL step.
//! - `a_signal_sent_before_the_first_wait_is_taken_by_it`: handlers in, signalled, then the task
//!   starts waiting and takes the held SIGTERM.
//! - `a_second_signal_exits_at_once_with_128_plus_signo`: the first signal's stop never ends; a
//!   second one flushes and exits with `128 + signo`.
//!
//! The real app needs a display and a session started from its page, which `cargo nextest` on
//! Linux CI has neither of, so the Tauri half (`AppHandle::exit` yielding
//! `RunEvent::ExitRequested`) is not driven here: `src/quit_signals.rs`'s `quit_on_signals` names
//! the source it rests on.

#![cfg(unix)]

#[cfg(test)]
mod quit {
    use std::collections::BTreeMap;
    use std::ffi::OsString;
    use std::future::Future;
    use std::io::Write as _;
    use std::path::{Path, PathBuf};
    use std::pin::{Pin, pin};
    use std::sync::Arc;
    use std::sync::mpsc::{Receiver, channel};
    use std::time::Duration;

    use async_io::{Timer, block_on};
    use clauseo_desktop::quit_signals_for_tests::{QuitSignals, exit_code};
    use clauseo_host::{Host, HostConfig};
    use clauseo_process::{
        AgentStdin, ExitReport, Handle, LaunchSpec, Launched, ManualTimer, PassthroughLauncher,
        ShutdownOutcome, SpawnOptions, spawn_in_tree,
    };
    use clauseo_protocol::{
        AgentId, CommandEnvelope, CommandId, CommandOutcome, DeliveryRequest, HostCommand,
        MatterSource, PromptBlock, Title,
    };
    use clauseo_settings::{Effective, Files as _, Locations, SystemFiles};
    use futures::future::{Either, select};
    use futures::io::{AsyncBufReadExt as _, AsyncReadExt as _, AsyncWriteExt as _, BufReader};
    use futures::stream::{BoxStream, StreamExt as _};
    use nix::errno::Errno;
    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;

    /// The variable that makes a run of this binary a role, and names its function.
    const ROLE: &str = "CLAUSEO_QUIT_ON_SIGNAL_ROLE";
    /// The scratch folder the app role works in.
    const SCRATCH: &str = "CLAUSEO_QUIT_ON_SIGNAL_SCRATCH";
    /// What marks each line a role says to the test (libtest writes its own lines too).
    const SAYS: &str = "role: ";
    /// The agent id the settings name.
    const AGENT: &str = "fake";
    /// How long a signalled role may take to exit, or a role to say its next line: the ladder
    /// ends this agent at its first step (it exits on its stdin's end), so this is margin.
    const EXIT_WITHIN: Duration = Duration::from_secs(20);
    /// How long a killed process may take to stop running, and the agent to write its child's pid.
    const GONE_WITHIN: Duration = Duration::from_secs(10);
    /// How often a bounded wait asks again.
    const POLL: Duration = Duration::from_millis(10);

    // ---- The cases. ----

    #[test]
    fn sigterm_stops_the_agent_and_its_child() {
        if in_a_role() {
            return;
        }
        let scratch = scratch();
        let mut app = Role::start("role_app", Some(scratch.path()));
        let ready = app.line();
        let pids: Vec<i32> = ready
            .strip_prefix("ready ")
            .unwrap_or_else(|| panic!("the app said {ready:?}"))
            .split(' ')
            .map(|pid| pid.parse().unwrap())
            .collect();
        let [agent, child] = pids[..] else {
            panic!("the app said {ready:?}");
        };
        let _reap = Reap(vec![agent, child]);
        assert!(running(agent), "the agent is not running");
        assert!(running(child), "the agent's child is not running");

        app.signal(Signal::SIGTERM);
        let report = app.exit();
        assert!(
            gone_within(child, GONE_WITHIN),
            "the agent's child runs on after the app's SIGTERM (the app ended as {report:?})"
        );
        assert!(
            gone_within(agent, GONE_WITHIN),
            "the agent runs on after the app's SIGTERM (the app ended as {report:?})"
        );
        assert_eq!(report.code(), Some(0), "the app ended as {report:?}");
    }

    #[test]
    fn the_agent_dies_at_the_ladder_s_sigterm_under_the_app_s_handlers() {
        if in_a_role() {
            return;
        }
        let mut role = Role::start("role_ladder", None);
        assert_eq!(role.line(), "terminated");
        let report = role.exit();
        assert_eq!(report.code(), Some(0), "the role ended as {report:?}");
    }

    #[test]
    fn a_signal_sent_before_the_first_wait_is_taken_by_it() {
        if in_a_role() {
            return;
        }
        let mut role = Role::start("role_pending", None);
        assert_eq!(role.line(), "listening");
        // Delivered once kill returns, to the handler, which holds it for the first wait.
        role.signal(Signal::SIGTERM);
        role.say("go");
        assert_eq!(role.line(), "first SIGTERM");
        let report = role.exit();
        assert_eq!(report.code(), Some(0), "the role ended as {report:?}");
    }

    #[test]
    fn a_second_signal_exits_at_once_with_128_plus_signo() {
        if in_a_role() {
            return;
        }
        let mut role = Role::start("role_twice", None);
        assert_eq!(role.line(), "ready");
        role.signal(Signal::SIGTERM);
        assert_eq!(role.line(), "first SIGTERM");
        role.signal(Signal::SIGINT);
        assert_eq!(role.line(), "flushed");
        let report = role.exit();
        assert_eq!(report.code(), Some(exit_code(Signal::SIGINT as i32)));
        assert_eq!(report.code(), Some(130));
    }

    // ---- The roles: in a normal run each returns at once. ----

    /// A host with `fake_agent` on the scratch folder, one prompt so the agent and its child run,
    /// then the signals' task; the first signal stops the host and the role exits 0.
    #[test]
    fn role_app() {
        let Some(signals) = role("role_app") else {
            return;
        };
        let root = PathBuf::from(std::env::var_os(SCRATCH).unwrap());
        let effective = Effective::load(&Locations::under_home(&root), &SystemFiles, &[]);
        let launcher = PassthroughLauncher::new(SpawnOptions::new().clear_env());
        let (host, join) = Host::start(HostConfig::new(effective, Arc::new(launcher))).unwrap();
        let work = root.join("work");
        let folder = block_on(host.register_folder(work.clone())).unwrap();
        let dispatch = |command: HostCommand| {
            let envelope = CommandEnvelope {
                command_id: CommandId::new(uuid::Uuid::now_v7()).unwrap(),
                command,
            };
            block_on(host.dispatch(envelope)).unwrap()
        };
        let CommandOutcome::MatterCreated { matter } = dispatch(HostCommand::CreateMatter {
            name: Title::new("signal").unwrap(),
            source: MatterSource::ExistingFolder { folder },
            client: None,
            next_date: None,
        }) else {
            panic!("no matter");
        };
        let CommandOutcome::ThreadCreated { thread } = dispatch(HostCommand::CreateThread {
            matter,
            agent: AgentId::new(AGENT).unwrap(),
        }) else {
            panic!("no thread");
        };
        let outcome = dispatch(HostCommand::SendPrompt {
            thread,
            content: vec![PromptBlock::Text {
                text: "Start a command.".to_owned(),
            }],
            delivery: DeliveryRequest::Queue,
        });
        assert!(
            matches!(outcome, CommandOutcome::PromptAccepted { .. }),
            "{outcome:?}"
        );
        let child = pid_written(&work.join("child.pid"));
        let agent = parent_of(child);
        for pid in [agent, child] {
            assert_default_actions(pid);
        }

        let taken = run_signals(signals);
        say(&format!("ready {agent} {child}"));
        assert_eq!(taken.recv().unwrap(), "SIGTERM");
        // What the app's stopper does for the host (`host_start::stop`): shut down, then join.
        block_on(host.shutdown()).unwrap();
        drop(join.join());
    }

    /// `fake_agent --hold` started through `spawn_in_tree` under the handlers, then the ladder on
    /// a manual timer: stdin's end does not end it, so the 2 s wait is fired; the 3 s wait after
    /// the group's SIGTERM is not, unless the agent still runs after 10 s of real time.
    #[test]
    fn role_ladder() {
        let Some(signals) = role("role_ladder") else {
            return;
        };
        let Launched { stderr, handle, .. } = spawn(&fake_agent(), &["--hold"]);
        let pid = i32::try_from(handle.pid()).unwrap();
        let mut reap = Reap(vec![pid]);
        let (tx, held) = futures::channel::oneshot::channel();
        let mut tx = Some(tx);
        let pumped = stderr.lines(move |line| {
            if line.contains("holding")
                && let Some(tx) = tx.take()
            {
                tx.send(()).unwrap_or_default();
            }
        });
        let timer = ManualTimer::new();
        let script = async {
            held.await.unwrap();
            assert_default_actions(pid);
            assert_eq!(timer.next_armed().await, Duration::from_secs(2));
            assert!(timer.fire());
            assert_eq!(timer.next_armed().await, Duration::from_secs(3));
            Timer::after(Duration::from_secs(10)).await;
            timer.fire();
            if timer.next_armed().await == Duration::from_secs(2) {
                timer.fire();
            }
        };
        let outcome = block_on(async {
            let ladder = pin!(handle.shutdown(&timer, None));
            let script = pin!(async { futures::join!(script, pumped) });
            match select(ladder, script).await {
                Either::Left((outcome, _)) => outcome,
                Either::Right((_, ladder)) => ladder.await,
            }
        });
        let ShutdownOutcome::Terminated(report) = outcome else {
            panic!("expected Terminated by the group's SIGTERM, got {outcome:?}");
        };
        assert_eq!(report.signal(), Some(Signal::SIGTERM as i32));
        // Reaped by the ladder: its pid may be reused, so nothing may kill it now.
        reap.0.clear();
        drop(signals);
        say("terminated");
    }

    /// Handlers in; says so, waits for the test's signal and its "go", then starts the task,
    /// whose first wait takes the held signal at once.
    #[test]
    fn role_pending() {
        let Some(signals) = role("role_pending") else {
            return;
        };
        say("listening");
        let mut go = String::new();
        std::io::stdin().read_line(&mut go).unwrap();
        assert_eq!(go.trim(), "go");
        let taken = run_signals(signals);
        say(&format!("first {}", taken.recv().unwrap()));
    }

    /// A first signal whose stop never ends, then a second that exits with `128 + signo`.
    #[test]
    fn role_twice() {
        let Some(signals) = role("role_twice") else {
            return;
        };
        let (_never, wait) = channel::<()>();
        drop(tauri::async_runtime::spawn(signals.run(
            |signal| say(&format!("first {signal}")),
            || say("flushed"),
        )));
        say("ready");
        // Only the second signal's exit ends the role.
        wait.recv().unwrap_or_default();
    }

    // ---- Helpers. ----

    /// Whether this run is a role (the cases then return at once).
    fn in_a_role() -> bool {
        std::env::var_os(ROLE).is_some()
    }

    /// The app's handlers, installed on Tauri's runtime as its setup does, when this run is the
    /// role `name`; otherwise `None`.
    fn role(name: &str) -> Option<QuitSignals> {
        if std::env::var_os(ROLE)? != name {
            return None;
        }
        Some(tauri::async_runtime::block_on(async { QuitSignals::listen() }).unwrap())
    }

    /// Runs `signals` on Tauri's runtime; the first signal's name comes out of the answer.
    fn run_signals(signals: QuitSignals) -> Receiver<&'static str> {
        let (first, taken) = channel();
        drop(tauri::async_runtime::spawn(signals.run(
            move |signal| first.send(signal).unwrap_or_default(),
            || {},
        )));
        taken
    }

    /// One line to the test, flushed.
    fn say(line: &str) {
        let mut out = std::io::stdout().lock();
        writeln!(out, "{SAYS}{line}").unwrap();
        out.flush().unwrap();
    }

    /// `fake_agent`'s binary.
    fn fake_agent() -> PathBuf {
        clauseo_fake_agent::bin_path()
            .unwrap_or_else(|e| panic!("{e} (run cargo build -p clauseo-fake_agent)"))
    }

    /// `program` with `args`, started through `spawn_in_tree`.
    fn spawn(program: &Path, args: &[&str]) -> Launched {
        let args = args.iter().map(OsString::from).collect();
        let spec = LaunchSpec::new(
            program.to_owned(),
            args,
            BTreeMap::new(),
            std::env::temp_dir(),
        )
        .unwrap();
        spawn_in_tree(&spec, &SpawnOptions::new()).unwrap()
    }

    /// A scratch folder holding the settings (the agent is `fake_agent` playing
    /// `prompt-child-in-group`) and the matter folder `work`; removed when dropped.
    fn scratch() -> tempfile::TempDir {
        let scratch = tempfile::Builder::new()
            .prefix("clauseo-desktop-quit-")
            .tempdir()
            .unwrap();
        let root = scratch.path();
        let settings = serde_json::json!({
            "agents": { AGENT: {
                "command": fake_agent().display().to_string(),
                "args": ["--scenario", "prompt-child-in-group"],
            } },
            "matters": { "default_location": root.join("matters").display().to_string() },
        });
        let files = SystemFiles;
        files
            .write_atomic(
                &root.join("data").join("settings.json"),
                settings.to_string().as_bytes(),
            )
            .unwrap();
        files
            .write_atomic(&root.join("work").join("brief.txt"), b"x\n")
            .unwrap();
        scratch
    }

    /// Waits up to [`GONE_WITHIN`] for `path` to hold a pid, and answers it.
    fn pid_written(path: &Path) -> i32 {
        let read = || {
            std::fs::read_to_string(path)
                .ok()
                .and_then(|text| text.trim().parse().ok())
        };
        poll_within(GONE_WITHIN, || read().is_some());
        read().unwrap_or_else(|| panic!("no pid in {} in time", path.display()))
    }

    /// The parent of `pid`, from `ps` (the same call on Linux and macOS).
    fn parent_of(pid: i32) -> i32 {
        let Launched {
            mut stdout,
            stderr,
            mut handle,
            ..
        } = spawn(
            Path::new("/bin/ps"),
            &["-o", "ppid=", "-p", &pid.to_string()],
        );
        let mut text = String::new();
        block_on(async { futures::join!(stdout.read_to_string(&mut text), stderr.drain()).0 })
            .unwrap();
        block_on(handle.exited());
        text.trim()
            .parse()
            .unwrap_or_else(|e| panic!("ps said {text:?}: {e}"))
    }

    /// On Linux, that `pid` has SIGHUP, SIGINT and SIGTERM neither blocked, ignored nor caught
    /// (`/proc/<pid>/status`'s `SigBlk`, `SigIgn` and `SigCgt`; bit `signo - 1`), and nothing
    /// blocked at all. Elsewhere there is nothing to read: the ladder's case checks the effect.
    fn assert_default_actions(pid: i32) {
        #[cfg(target_os = "linux")]
        {
            let status = std::fs::read_to_string(format!("/proc/{pid}/status")).unwrap();
            let mask = |field: &str| {
                let hex = status
                    .lines()
                    .find_map(|line| line.strip_prefix(field))
                    .unwrap_or_else(|| panic!("no {field} in {status}"));
                u64::from_str_radix(hex.trim(), 16).unwrap()
            };
            let quit = [Signal::SIGHUP, Signal::SIGINT, Signal::SIGTERM]
                .iter()
                .fold(0u64, |bits, signal| bits | 1 << (*signal as i32 - 1));
            assert_eq!(mask("SigBlk:"), 0, "{pid} starts with signals blocked");
            assert_eq!(mask("SigIgn:") & quit, 0, "{pid} ignores a quit signal");
            assert_eq!(mask("SigCgt:") & quit, 0, "{pid} catches a quit signal");
        }
        #[cfg(not(target_os = "linux"))]
        let _: i32 = pid;
    }

    /// Whether `pid` runs: it exists and, on Linux, is no zombie (a zombie whose parent is not
    /// this test, which a reaper reaps on its own time, is not running).
    fn running(pid: i32) -> bool {
        if kill(Pid::from_raw(pid), None) == Err(Errno::ESRCH) {
            return false;
        }
        #[cfg(target_os = "linux")]
        if let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat"))
            && let Some((_, rest)) = stat.rsplit_once(") ")
            && rest.starts_with('Z')
        {
            return false;
        }
        true
    }

    /// Whether `pid` stops running within `bound`, asked every [`POLL`].
    fn gone_within(pid: i32, bound: Duration) -> bool {
        poll_within(bound, || !running(pid))
    }

    /// Asks `done` every [`POLL`] until it holds or `bound` has passed; answers the last answer.
    fn poll_within(bound: Duration, mut done: impl FnMut() -> bool) -> bool {
        for _ in 0..bound.as_millis() / POLL.as_millis() {
            if done() {
                return true;
            }
            block_on(Timer::after(POLL));
        }
        done()
    }

    /// `future`'s output, which must come within [`EXIT_WITHIN`].
    fn within<T>(what: &str, future: impl Future<Output = T>) -> T {
        match block_on(select(pin!(future), Timer::after(EXIT_WITHIN))) {
            Either::Left((output, _)) => output,
            Either::Right(_) => panic!("no {what} in {} s", EXIT_WITHIN.as_secs()),
        }
    }

    /// SIGKILLs the pids still running when dropped, so a failed case leaves nothing behind.
    struct Reap(Vec<i32>);

    impl Drop for Reap {
        fn drop(&mut self) {
            for &pid in &self.0 {
                if running(pid) {
                    kill(Pid::from_raw(pid), Signal::SIGKILL).unwrap_or_default();
                }
            }
        }
    }

    /// A role's process: this test binary again, running one role function. Its stderr goes to
    /// the test's, each line marked; a role dropped still running is killed with its tree.
    struct Role {
        lines: BoxStream<'static, std::io::Result<String>>,
        stdin: AgentStdin,
        handle: Handle,
    }

    impl Role {
        fn start(name: &str, scratch: Option<&Path>) -> Role {
            let mut env = BTreeMap::from([(OsString::from(ROLE), OsString::from(name))]);
            if let Some(scratch) = scratch {
                env.insert(OsString::from(SCRATCH), OsString::from(scratch));
            }
            // Its path in the test crate, as `--exact` wants it.
            let test = format!("quit::{name}");
            let args = ["--exact", &test, "--nocapture", "--test-threads=1"]
                .map(OsString::from)
                .to_vec();
            let program = std::env::current_exe().unwrap();
            let cwd = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            let spec = LaunchSpec::new(program, args, env, cwd).unwrap();
            let Launched {
                stdin,
                stdout,
                stderr,
                handle,
                ..
            } = spawn_in_tree(&spec, &SpawnOptions::new()).unwrap();
            std::thread::spawn(move || {
                block_on(stderr.lines(|line| eprintln!("{SAYS}{line}"))).unwrap_or_default();
            });
            let lines: Pin<Box<_>> = Box::pin(BufReader::new(stdout).lines());
            Role {
                lines: lines.boxed(),
                stdin,
                handle,
            }
        }

        /// The role's next line to the test; libtest's own lines are skipped.
        fn line(&mut self) -> String {
            let mut skipped = Vec::new();
            loop {
                match within("line from the role", self.lines.next()) {
                    Some(line) => {
                        let line = line.unwrap();
                        // libtest's "test <name> ... " can come first on the line.
                        if let Some((_, said)) = line.split_once(SAYS) {
                            return said.to_owned();
                        }
                        skipped.push(line);
                    }
                    None => panic!(
                        "the role ended with no line after {skipped:?}: {:?}",
                        within("exit of the role", self.handle.exited())
                    ),
                }
            }
        }

        /// A line to the role's stdin.
        fn say(&mut self, line: &str) {
            block_on(async {
                self.stdin.write_all(format!("{line}\n").as_bytes()).await?;
                self.stdin.flush().await
            })
            .unwrap();
        }

        fn signal(&self, signal: Signal) {
            let pid = Pid::from_raw(i32::try_from(self.handle.pid()).unwrap());
            kill(pid, signal).unwrap();
        }

        /// How the role exited, which it must within [`EXIT_WITHIN`].
        fn exit(&mut self) -> ExitReport {
            within("exit of the role", self.handle.exited())
        }
    }
}
