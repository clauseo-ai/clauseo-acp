//! Runs a command in the agent jail, as an agent would run, for the gates (G3, G8b, G10) and
//! by hand:
//!
//! ```text
//! cargo run -p clauseo-sandbox --example run_sandboxed -- --matter <dir> --state <dir>
//!     [--runtime <p>]... [--read <p>]... [--hidden <p>]... [--proxy-port <n>]
//!     [--loopback <port>]... [--browser-sockets] [--stdin] [--print-spec]
//!     -- <cmd> [args]
//! ```
//!
//! `cmd` is an absolute path and runs in the matter with stdin closed; its stdout and stderr
//! come through. `--stdin` feeds this process's stdin to `cmd` instead, for a process that
//! speaks JSON-RPC over stdio (agent-browser's MCP server, `clauseo-mcp browser-guard`); the
//! child's stdin closes when ours does. `--loopback <port>` (repeatable) lets the command reach
//! those loopback ports, for Chrome's and agent-browser's CDP ports; none means no loopback
//! beyond the proxy ports. `--browser-sockets` (Unix) makes the browser's socket folder,
//! `/tmp/clauseo-ab-<id>`, allows it in the jail, and sets `AGENT_BROWSER_SOCKET_DIR` to it
//! for `cmd`; the folder's path is printed on stderr and the folder is removed when the
//! command has exited. Each denial prints as one JSON line on stderr, for a second after the
//! command ends (the watcher's lines arrive within a few tens of milliseconds). `--print-spec`
//! first prints the profile and its `-D` arguments as one JSON object on stdout. The exit code
//! is the command's (128 plus the signal when a signal ended it); 2 for a usage error or a
//! refused policy. macOS only until S15 adds Windows.

use std::process::ExitCode;

fn main() -> ExitCode {
    #[cfg(target_os = "macos")]
    {
        jail::main()
    }
    #[cfg(not(target_os = "macos"))]
    {
        use std::io::Write as _;
        let message = "run_sandboxed: no jail on this OS yet (macOS only; S15 adds Windows)\n";
        if std::io::stderr().write_all(message.as_bytes()).is_err() {
            return ExitCode::from(3);
        }
        ExitCode::from(2)
    }
}

#[cfg(target_os = "macos")]
mod jail {
    use std::collections::BTreeMap;
    use std::ffi::OsString;
    use std::io::Write as _;
    use std::path::{Path, PathBuf};
    use std::process::ExitCode;
    use std::sync::Arc;
    use std::time::Duration;

    use clauseo_process::{AgentStdin, LaunchSpec, Launched, Launcher, SpawnOptions};
    use clauseo_sandbox::{
        BrowserSocketDir, Denial, DenialSink, Loopback, Policy, SeatbeltLauncher,
    };
    use futures::io::{AllowStdIo, AsyncWriteExt as _};

    /// How long denials keep printing after the command ends.
    const DENIAL_GRACE: Duration = Duration::from_secs(1);

    /// The parsed command line.
    #[derive(Default)]
    struct Args {
        matter: Option<PathBuf>,
        state: Option<PathBuf>,
        runtime: Vec<PathBuf>,
        read: Vec<PathBuf>,
        hidden: Vec<PathBuf>,
        proxy_port: Option<u16>,
        loopback: Vec<u16>,
        browser_sockets: bool,
        stdin: bool,
        print_spec: bool,
        command: Vec<OsString>,
    }

    const USAGE: &str = "usage: run_sandboxed --matter <dir> --state <dir> [--runtime <p>]... \
                         [--read <p>]... [--hidden <p>]... [--proxy-port <n>] \
                         [--loopback <port>]... [--browser-sockets] [--stdin] [--print-spec] \
                         -- <cmd> [args]";

    /// A loopback port: a number from 1 to 65535.
    fn parse_port(name: &str, value: &Path) -> Result<u16, String> {
        let text = value.to_string_lossy();
        match text.parse::<u16>() {
            Ok(0) => Err(format!(
                "bad port {text:?} for {name}: port 0 is not a port"
            )),
            Ok(port) => Ok(port),
            Err(e) => Err(format!("bad port {text:?} for {name}: {e}")),
        }
    }

    fn parse(mut raw: impl Iterator<Item = OsString>) -> Result<Args, String> {
        let mut args = Args::default();
        while let Some(flag) = raw.next() {
            let flag = flag.to_string_lossy().into_owned();
            let mut value = |name: &str| {
                raw.next()
                    .map(PathBuf::from)
                    .ok_or_else(|| format!("{name} needs a value"))
            };
            match flag.as_str() {
                "--matter" => args.matter = Some(value("--matter")?),
                "--state" => args.state = Some(value("--state")?),
                "--runtime" => args.runtime.push(value("--runtime")?),
                "--read" => args.read.push(value("--read")?),
                "--hidden" => args.hidden.push(value("--hidden")?),
                "--proxy-port" => {
                    let port = value("--proxy-port")?;
                    let port = port.to_string_lossy();
                    args.proxy_port = Some(
                        port.parse()
                            .map_err(|e| format!("bad port {port:?}: {e}"))?,
                    );
                }
                "--loopback" => args
                    .loopback
                    .push(parse_port("--loopback", &value("--loopback")?)?),
                "--browser-sockets" => args.browser_sockets = true,
                "--stdin" => args.stdin = true,
                "--print-spec" => args.print_spec = true,
                "--" => {
                    args.command = raw.collect();
                    break;
                }
                other => return Err(format!("unknown argument {other:?}")),
            }
        }
        if args.command.is_empty() {
            return Err("no command after --".to_owned());
        }
        Ok(args)
    }

    /// Prints each denial as one JSON line on stderr.
    struct PrintSink;

    impl DenialSink for PrintSink {
        fn denied(&self, d: Denial) {
            if let Ok(line) = serde_json::to_string(&d) {
                say(&format!("{line}\n"));
            }
        }
    }

    /// Writes `text` to stderr; a closed stderr is not worth failing over.
    fn say(text: &str) {
        if std::io::stderr().write_all(text.as_bytes()).is_err() {
            // Nowhere left to report it.
        }
    }

    pub(super) fn main() -> ExitCode {
        let args = match parse(std::env::args_os().skip(1)) {
            Ok(args) => args,
            Err(why) => {
                say(&format!("run_sandboxed: {why}\n{USAGE}\n"));
                return ExitCode::from(2);
            }
        };
        match run(args) {
            Ok(code) => code,
            Err(why) => {
                say(&format!("run_sandboxed: {why}\n"));
                ExitCode::from(2)
            }
        }
    }

    fn run(args: Args) -> Result<ExitCode, String> {
        let (Some(matter), Some(state)) = (args.matter, args.state) else {
            return Err(format!("--matter and --state are required\n{USAGE}"));
        };
        let mut builder = Policy::builder(&matter, state);
        for p in args.runtime {
            builder = builder.runtime(p);
        }
        for p in args.read {
            builder = builder.read(p);
        }
        for p in args.hidden {
            builder = builder.hidden(p);
        }
        if let Some(port) = args.proxy_port {
            builder = builder.proxy_port(port);
        }
        if !args.loopback.is_empty() {
            builder = builder.loopback(Loopback::Ports(args.loopback));
        }
        // The folder lives until the command has exited: it is dropped after `drive`.
        let socket_dir = if args.browser_sockets {
            let dir = BrowserSocketDir::create().map_err(|e| format!("--browser-sockets: {e}"))?;
            builder = builder.browser_socket_dir(&dir);
            say(&format!(
                "run_sandboxed: browser socket folder {}\n",
                dir.path().display()
            ));
            Some(dir)
        } else {
            None
        };
        let policy = builder.build().map_err(|e| e.to_string())?;
        let cwd = policy.matter().to_owned();
        let launcher = SeatbeltLauncher::new(policy, SpawnOptions::new(), Arc::new(PrintSink));
        let mut command = args.command.into_iter();
        let program = PathBuf::from(command.next().unwrap_or_default());
        let mut env = BTreeMap::new();
        if let Some(dir) = &socket_dir {
            env.insert(
                OsString::from("AGENT_BROWSER_SOCKET_DIR"),
                dir.path().as_os_str().to_owned(),
            );
        }
        let spec =
            LaunchSpec::new(program, command.collect(), env, cwd).map_err(|e| e.to_string())?;
        if args.print_spec {
            print_spec(&launcher)?;
        }
        let launched = launcher.launch(&spec).map_err(|e| e.to_string())?;
        let code = async_io::block_on(drive(launched, args.stdin));
        // The watcher's lines come a little after the denial itself.
        async_io::block_on(async_io::Timer::after(DENIAL_GRACE));
        drop(launcher);
        drop(socket_dir);
        Ok(code)
    }

    fn print_spec(launcher: &SeatbeltLauncher) -> Result<(), String> {
        let profile = launcher.profile();
        let defines: Vec<String> = profile
            .params()
            .iter()
            .map(|(name, path)| format!("{name}={}", path.display()))
            .collect();
        let spec = serde_json::json!({
            "program": "/usr/bin/sandbox-exec",
            "tag": launcher.tag(),
            "profile": profile.text(),
            "defines": defines,
        });
        let mut out = std::io::stdout().lock();
        writeln!(out, "{spec}")
            .and_then(|()| out.flush())
            .map_err(|e| e.to_string())
    }

    /// Copies this process's stdin into the command's stdin, then closes the command's stdin at
    /// the end. Runs on its own thread: it may wait on a read after the command has exited.
    fn feed(mut to: AgentStdin) {
        let mut from = AllowStdIo::new(std::io::stdin());
        let copied = async_io::block_on(async {
            futures::io::copy(&mut from, &mut to).await?;
            to.close().await
        });
        if let Err(error) = copied {
            say(&format!("run_sandboxed: stdin: {error}\n"));
        }
    }

    /// Passes the command's output through and waits for its exit. Its stdin is closed at once,
    /// or fed from this process's stdin with `pass_stdin`.
    async fn drive(launched: Launched, pass_stdin: bool) -> ExitCode {
        let Launched {
            stdin,
            stdout,
            stderr,
            mut handle,
            ..
        } = launched;
        if pass_stdin {
            // Detached: a read still waiting when the command exits ends with the process.
            drop(std::thread::spawn(move || feed(stdin)));
        } else {
            drop(stdin);
        }
        let mut sink = AllowStdIo::new(std::io::stdout());
        let out = futures::io::copy(stdout, &mut sink);
        let err = stderr.lines(|line| say(&format!("{line}\n")));
        let (copied, pumped, report) = futures::join!(out, err, handle.exited());
        if let Err(error) = copied.and(pumped) {
            say(&format!("run_sandboxed: {error}\n"));
        }
        let code = match (report.code(), report.signal()) {
            (Some(code), _) => code,
            (None, Some(signal)) => 128 + signal,
            (None, None) => 1,
        };
        ExitCode::from(u8::try_from(code).unwrap_or(1))
    }
}
