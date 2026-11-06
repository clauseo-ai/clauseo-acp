//! Runs a command in the agent jail, as an agent would run, for the gates (G3, G8b, G10) and
//! by hand:
//!
//! ```text
//! cargo run -p clauseo-sandbox --example run_sandboxed -- --matter <dir> --state <dir>
//!     [--runtime <p>]... [--read <p>]... [--hidden <p>]... [--proxy-port <n>]
//!     [--loopback <port>]... [--browser-sockets] [--env K=V]... [--env-file <p>]
//!     [--stdin] [--print-spec] -- <cmd> [args]
//! ```
//!
//! `cmd` is an absolute path and runs in the matter with stdin closed; its stdout and stderr come
//! through. `--env K=V` (repeatable) adds one variable to `cmd`'s environment; a later `K` wins.
//! `--env-file <path>` (once) reads one `K=V` per line from a file only its owner can read, and
//! removes the file before it parses the lines; `--env` values override the file's. `--stdin` feeds
//! this process's stdin to `cmd` instead, for a process that speaks JSON-RPC over stdio
//! (agent-browser's MCP server, `clauseo-mcp browser-guard`); the child's stdin closes when ours
//! does. `--loopback <port>` (repeatable) lets the command reach those loopback ports, for Chrome's
//! and agent-browser's CDP ports; none means no loopback beyond the proxy ports.
//! `--browser-sockets` (Unix) makes the browser's socket folder, `/tmp/clauseo-ab-<id>`, allows it
//! in the jail, and sets `AGENT_BROWSER_SOCKET_DIR` to it for `cmd`; the folder's path is printed
//! on stderr and the folder is removed when the command has exited. `--browser-sockets` refuses an
//! `AGENT_BROWSER_SOCKET_DIR` from `--env` or `--env-file`. Each denial prints as one JSON line on
//! stderr, for a second after the command ends (the watcher's lines arrive within a few tens of
//! milliseconds). `--print-spec` first prints the profile and its `-D` arguments as one JSON object
//! on stdout. The exit code is the command's (128 plus the signal when a signal ended it); 2 for a
//! usage error or a refused policy. macOS only until S15 adds Windows.

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
    use std::fs::File;
    use std::io::{Read, Write as _};
    use std::path::{Path, PathBuf};
    use std::process::ExitCode;
    use std::sync::Arc;
    use std::time::Duration;

    use clauseo_process::{AgentStdin, LaunchSpec, Launched, Launcher, SpawnOptions};
    use clauseo_sandbox::{
        BrowserSocketDir, Denial, DenialSink, Loopback, Policy, SeatbeltLauncher,
    };
    use futures::io::{AllowStdIo, AsyncWriteExt as _};
    use rustix::fs::{FileType, Mode, OFlags};
    use rustix::io::Errno;
    use rustix::process::geteuid;

    /// How long denials keep printing after the command ends.
    const DENIAL_GRACE: Duration = Duration::from_secs(1);

    /// Most bytes an `--env-file` may hold.
    const ENV_FILE_CAP: u64 = 64 * 1024;

    /// The variable `--browser-sockets` sets, which `--env` and `--env-file` may not.
    const SOCKET_DIR_VAR: &str = "AGENT_BROWSER_SOCKET_DIR";

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
        env: Vec<(String, String)>,
        env_file: Option<PathBuf>,
        stdin: bool,
        print_spec: bool,
        command: Vec<OsString>,
    }

    const USAGE: &str = "usage: run_sandboxed --matter <dir> --state <dir> [--runtime <p>]... \
                         [--read <p>]... [--hidden <p>]... [--proxy-port <n>] \
                         [--loopback <port>]... [--browser-sockets] [--env K=V]... \
                         [--env-file <p>] [--stdin] [--print-spec] -- <cmd> [args]";

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

    /// Why `key` and `value` cannot be one variable, or `None` when they can. The reason
    /// never names the value. (`=` cannot be in `key`: the caller splits at the first one.)
    fn bad_pair(key: &str, value: &str) -> Option<&'static str> {
        if key.is_empty() {
            Some("needs a non-empty key before the '='")
        } else if key.contains('\0') || value.contains('\0') {
            Some("has a NUL byte")
        } else {
            None
        }
    }

    /// One `K=V` from `--env`: the key is what precedes the first `=`, and the value keeps any
    /// `=` after it.
    fn parse_env_pair(pair: &str) -> Result<(String, String), String> {
        let Some((key, value)) = pair.split_once('=') else {
            return Err("--env needs K=V".to_owned());
        };
        match bad_pair(key, value) {
            Some(why) => Err(format!("--env {why}")),
            None => Ok((key.to_owned(), value.to_owned())),
        }
    }

    /// The variables in an `--env-file`'s text: one `K=V` a line, with no quoting or escapes.
    /// Blank lines and lines starting with `#` are skipped, and a trailing `\r` is dropped. An
    /// error names the line number only, never the line.
    fn parse_env_file_text(text: &str) -> Result<Vec<(String, String)>, String> {
        let mut pairs = Vec::new();
        for (index, line) in text.split('\n').enumerate() {
            let line = line.strip_suffix('\r').unwrap_or(line);
            if line.trim().is_empty() || line.starts_with('#') {
                continue;
            }
            let number = index + 1;
            let Some((key, value)) = line.split_once('=') else {
                return Err(format!("--env-file line {number} is not K=V"));
            };
            match bad_pair(key, value) {
                Some(why) => return Err(format!("--env-file line {number} {why}")),
                None => pairs.push((key.to_owned(), value.to_owned())),
            }
        }
        Ok(pairs)
    }

    /// Reads at most `ENV_FILE_CAP` bytes from `from`; more is an error.
    fn read_capped(from: impl Read) -> Result<Vec<u8>, String> {
        let mut bytes = Vec::new();
        from.take(ENV_FILE_CAP + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| format!("--env-file: {e}"))?;
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > ENV_FILE_CAP {
            return Err("--env-file is larger than 64 KiB".to_owned());
        }
        Ok(bytes)
    }

    /// Removes the `--env-file` at `path`, a link itself and never what it points to. A file
    /// that stays would keep the secret, so a failure stops the run.
    #[expect(
        clippy::disallowed_methods,
        reason = "the --env-file is removed right after it is read, so no secret outlives the read"
    )]
    fn remove_secret(path: &Path) -> Result<(), String> {
        std::fs::remove_file(path).map_err(|e| format!("--env-file could not be removed: {e}"))
    }

    /// Reads the `--env-file` at `path`: a regular file owned by us with no group or other
    /// bits, opened without following a link. Once it passes those checks it is removed, before
    /// anything is parsed. A file that fails them is left alone.
    fn read_env_file(path: &Path) -> Result<String, String> {
        const REFUSED: &str = "--env-file must be a regular file only you can read";
        // O_NONBLOCK keeps a FIFO from holding the open; the fstat below refuses it.
        let flags = OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC;
        let fd = rustix::fs::open(path, flags, Mode::empty()).map_err(|e| match e {
            Errno::LOOP => REFUSED.to_owned(),
            other => format!("--env-file: {other}"),
        })?;
        let stat = rustix::fs::fstat(&fd).map_err(|e| format!("--env-file: {e}"))?;
        let regular = FileType::from_raw_mode(stat.st_mode) == FileType::RegularFile;
        let group_or_other = stat.st_mode & 0o077;
        if !regular || group_or_other != 0 || stat.st_uid != geteuid().as_raw() {
            return Err(REFUSED.to_owned());
        }
        let mut file = File::from(fd);
        let bytes = read_capped(&mut file);
        // The file is ours and passed its checks, so it is spent whatever the read found.
        remove_secret(path)?;
        String::from_utf8(bytes?)
            .map_err(|e| format!("--env-file is not UTF-8: {}", e.utf8_error()))
    }

    /// The command's environment: the `--env-file` variables, then the `--env` pairs, so a
    /// later key wins. With `--browser-sockets` the folder's variable is set by the caller,
    /// so a given one is a refusal.
    fn launch_env(
        from_file: Vec<(String, String)>,
        from_args: Vec<(String, String)>,
        browser_sockets: bool,
    ) -> Result<BTreeMap<OsString, OsString>, String> {
        let mut env = BTreeMap::new();
        for (key, value) in from_file.into_iter().chain(from_args) {
            if browser_sockets && key == SOCKET_DIR_VAR {
                return Err(format!(
                    "--browser-sockets sets {SOCKET_DIR_VAR}; drop it from --env or --env-file"
                ));
            }
            env.insert(OsString::from(key), OsString::from(value));
        }
        Ok(env)
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
                "--env" => {
                    let pair = value("--env")?;
                    let text = pair
                        .to_str()
                        .ok_or_else(|| "--env must be UTF-8".to_owned())?;
                    args.env.push(parse_env_pair(text)?);
                }
                "--env-file" => {
                    if args.env_file.is_some() {
                        return Err("--env-file may be given once".to_owned());
                    }
                    args.env_file = Some(value("--env-file")?);
                }
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
        // Read first: a file that passed its checks is spent even if a later step refuses.
        let from_file = match &args.env_file {
            Some(path) => parse_env_file_text(&read_env_file(path)?)?,
            None => Vec::new(),
        };
        let mut env = launch_env(from_file, args.env, args.browser_sockets)?;
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
        if let Some(dir) = &socket_dir {
            env.insert(
                OsString::from(SOCKET_DIR_VAR),
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

    #[cfg(test)]
    mod tests {
        use std::ffi::OsStr;

        use super::*;

        fn pair(key: &str, value: &str) -> (String, String) {
            (key.to_owned(), value.to_owned())
        }

        #[test]
        fn an_env_pair_splits_at_the_first_equals() {
            assert_eq!(parse_env_pair("FOO=bar").unwrap(), pair("FOO", "bar"));
            assert_eq!(parse_env_pair("FOO=").unwrap(), pair("FOO", ""));
            assert_eq!(parse_env_pair("FOO=a=b=").unwrap(), pair("FOO", "a=b="));
        }

        #[test]
        fn a_bad_env_pair_is_refused_without_its_value() {
            for bad in ["FOO", "=bar", "FOO\0=1", "FOO=secret\0x"] {
                let why = parse_env_pair(bad).expect_err(bad);
                assert!(!why.contains("secret"), "{why}");
                assert!(why.starts_with("--env "), "{why}");
            }
        }

        #[test]
        fn env_file_lines_are_pairs_with_blanks_and_comments_skipped() {
            let text = "\n# a note\n   \nFOO=bar\n#BAR=no\nBAZ=a=b\n";
            assert_eq!(
                parse_env_file_text(text).unwrap(),
                vec![pair("FOO", "bar"), pair("BAZ", "a=b")]
            );
        }

        #[test]
        fn env_file_crlf_lines_lose_their_carriage_return() {
            assert_eq!(
                parse_env_file_text("FOO=bar\r\nBAZ=1\r").unwrap(),
                vec![pair("FOO", "bar"), pair("BAZ", "1")]
            );
        }

        #[test]
        fn a_bad_env_file_line_is_named_by_number_only() {
            for (text, line) in [
                ("A=1\nNOEQUALS\n", 2),
                ("A=1\n=secret\n", 2),
                ("A=1\n\nB=s\0x\n", 3),
            ] {
                let why = parse_env_file_text(text).expect_err(text);
                assert!(why.contains(&format!("line {line}")), "{why}");
                assert!(
                    !why.contains("secret") && !why.contains("NOEQUALS"),
                    "{why}"
                );
            }
        }

        #[test]
        fn the_env_file_cap_is_64_kib() {
            let at_cap = vec![b'a'; 64 * 1024];
            assert_eq!(read_capped(at_cap.as_slice()).unwrap().len(), 64 * 1024);
            let over = vec![b'a'; 64 * 1024 + 1];
            assert!(read_capped(over.as_slice()).is_err());
        }

        #[test]
        fn the_last_of_a_key_wins_and_args_beat_the_file() {
            let args = vec![pair("FOO", "a"), pair("FOO", "b")];
            let env = launch_env(Vec::new(), args, false).unwrap();
            assert_eq!(env.get(OsStr::new("FOO")), Some(&OsString::from("b")));
            let env = launch_env(vec![pair("FOO", "1")], vec![pair("FOO", "2")], false).unwrap();
            assert_eq!(env.get(OsStr::new("FOO")), Some(&OsString::from("2")));
        }

        #[test]
        fn browser_sockets_refuses_a_given_socket_dir() {
            let given = vec![pair(SOCKET_DIR_VAR, "x")];
            assert!(launch_env(Vec::new(), given.clone(), true).is_err());
            assert!(launch_env(given.clone(), Vec::new(), true).is_err());
            assert!(launch_env(Vec::new(), given, false).is_ok());
        }
    }
}
