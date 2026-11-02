//! Runs a command in the agent jail, as an agent would run, for the gates (G3, G8b, G10) and
//! by hand:
//!
//! ```text
//! cargo run -p clauseo-sandbox --example run_sandboxed -- --matter <dir> --state <dir>
//!     [--runtime <p>]... [--read <p>]... [--hidden <p>]... [--proxy-port <n>] [--print-spec]
//!     -- <cmd> [args]
//! ```
//!
//! `cmd` is an absolute path and runs in the matter with stdin closed; its stdout and stderr
//! come through. Each denial prints as one JSON line on stderr, for a second after the command
//! ends (the watcher's lines arrive within a few tens of milliseconds). `--print-spec` first
//! prints the profile and its `-D` arguments as one JSON object on stdout. The exit code is
//! the command's (128 plus the signal when a signal ended it); 2 for a usage error or a
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
    use std::path::PathBuf;
    use std::process::ExitCode;
    use std::sync::Arc;
    use std::time::Duration;

    use clauseo_process::{LaunchSpec, Launched, Launcher, SpawnOptions};
    use clauseo_sandbox::{Denial, DenialSink, Policy, SeatbeltLauncher};
    use futures::io::AllowStdIo;

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
        print_spec: bool,
        command: Vec<OsString>,
    }

    const USAGE: &str = "usage: run_sandboxed --matter <dir> --state <dir> [--runtime <p>]... \
                         [--read <p>]... [--hidden <p>]... [--proxy-port <n>] [--print-spec] \
                         -- <cmd> [args]";

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
        let policy = builder.build().map_err(|e| e.to_string())?;
        let cwd = policy.matter().to_owned();
        let launcher = SeatbeltLauncher::new(policy, SpawnOptions::new(), Arc::new(PrintSink));
        let mut command = args.command.into_iter();
        let program = PathBuf::from(command.next().unwrap_or_default());
        let spec = LaunchSpec::new(program, command.collect(), BTreeMap::new(), cwd)
            .map_err(|e| e.to_string())?;
        if args.print_spec {
            print_spec(&launcher)?;
        }
        let launched = launcher.launch(&spec).map_err(|e| e.to_string())?;
        let code = async_io::block_on(drive(launched));
        // The watcher's lines come a little after the denial itself.
        async_io::block_on(async_io::Timer::after(DENIAL_GRACE));
        drop(launcher);
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

    /// Passes the command's output through and waits for its exit.
    async fn drive(launched: Launched) -> ExitCode {
        let Launched {
            stdin,
            stdout,
            stderr,
            mut handle,
            ..
        } = launched;
        drop(stdin);
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
