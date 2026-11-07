//! The VM rows of S15 PR7 (tier B's network), run on a copy of `clauseo-win-01` after set-up, in
//! the desktop session through `run2.sh` (`CreateProcessWithLogonW` fails from session 0):
//!
//! ```text
//! cargo build -p clauseo-sandbox --example s15_wfp_vm
//! copy s15_wfp_vm.exe C:\s15\bin\        (a folder the pool users can read and run)
//! C:\s15\bin\s15_wfp_vm.exe status
//! C:\s15\bin\s15_wfp_vm.exe rows [--foreign <port>] [--share \\host\guest]
//! C:\s15\bin\s15_wfp_vm.exe listen <port>  (as a second Windows user, for the two-user row)
//! ```
//!
//! `status` checks set-up's filters for each pool user the machine record lists, as a launch
//! does. `rows` takes the record's first two pool users as matters A and B, starts an S16 proxy
//! for each, a stand-in relay listener for A and an unlisted one, opens each matter's permit as
//! a launch would, and runs the probe (`probe.rs`) as each pool user under the (B) token
//! (`launch.rs`). Each row is one JSON line on stdout with what was expected and what happened;
//! the last line counts the passes. `--foreign` adds the two-user row: a port a second Windows
//! user's `listen` holds in 61080-61179, which A's agent must not reach. `--share` adds a guest
//! share's row beside `\\localhost\C$`'s (0049's network-share risk).
//!
//! Every permit is closed and every proxy stopped when it ends. Off Windows it says so.

#[cfg(windows)]
use std::error::Error;
use std::io::{self, Write};

#[cfg(windows)]
#[path = "s15_wfp_vm/launch.rs"]
mod launch;
#[cfg(windows)]
#[path = "s15_wfp_vm/probe.rs"]
mod probe;

/// Writes `line` and a newline to stdout.
fn say(line: &str) {
    let mut out = io::stdout();
    if writeln!(out, "{line}").and_then(|()| out.flush()).is_err() {
        drop(writeln!(io::stderr(), "s15_wfp_vm: stdout is gone"));
    }
}

#[cfg(not(windows))]
fn main() {
    say("s15_wfp_vm runs on Windows only");
}

#[cfg(windows)]
fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (command, rest) = args
        .split_first()
        .ok_or("usage: status | rows | listen <port>")?;
    let exe = std::env::current_exe()?;
    match command.as_str() {
        // The agent's side and its runner give the probe's bits as their exit code.
        "probe" => std::process::exit(as_exit(probe::run(rest))),
        "runner" => {
            let code = launch::runner(&exe, rest).unwrap_or_else(|why| {
                drop(writeln!(io::stderr(), "runner: {why}"));
                2
            });
            std::process::exit(as_exit(code))
        }
        "status" => vm::status(),
        "rows" => vm::rows(&exe, rest),
        "listen" => vm::listen(rest),
        other => Err(format!("unknown command {other:?}").into()),
    }
}

/// A `u32` exit code as `exit` takes it.
#[cfg(windows)]
fn as_exit(code: u32) -> i32 {
    i32::from_ne_bytes(code.to_ne_bytes())
}

#[cfg(windows)]
mod vm {
    use std::error::Error;
    use std::net::TcpListener;
    use std::path::Path;
    use std::sync::Arc;

    use base64::Engine;
    use clauseo_sandbox::windows::sids::Sid;
    use clauseo_sandbox::windows::wfp::{LaunchPermit, Namespace};
    use clauseo_sandbox::windows::{sandbox_data_dir, sandbox_pool_sids};
    use clauseo_sandbox::{
        Allowlist, Denial, DenialSink, ProxyConfig, ProxyHandle, Reachability, Upstream, ports,
        start_proxy,
    };

    use super::{launch, probe, say};
    use Expect::{Blocked, Measured, Reached};

    /// Denials go nowhere: the rows say what was reached.
    struct Quiet;

    impl DenialSink for Quiet {
        fn denied(&self, _denial: Denial) {}
        fn upstream(&self, _reach: Reachability) {}
    }

    /// What a row expects of its step.
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Expect {
        Reached,
        Blocked,
        /// A residual: measured and reported, neither passing nor failing.
        Measured,
    }

    /// One row: who, what it shows, the probe step, what it expects.
    struct Row {
        user: &'static str,
        what: String,
        step: String,
        expect: Expect,
    }

    fn row(user: &'static str, what: &str, step: String, expect: Expect) -> Row {
        Row {
            user,
            what: what.to_owned(),
            step,
            expect,
        }
    }

    pub(super) fn status() -> Result<(), Box<dyn Error>> {
        for sid in sandbox_pool_sids()? {
            let checked = LaunchPermit::open_ports(&Namespace::production(), &sid, &[]);
            let line = match checked {
                Ok(permit) => {
                    permit.close();
                    serde_json::json!({ "pool_user": sid.as_str(), "filtered": true })
                }
                Err(error) => serde_json::json!({
                    "pool_user": sid.as_str(),
                    "filtered": false,
                    "why": error.to_string(),
                }),
            };
            say(&line.to_string());
        }
        Ok(())
    }

    pub(super) fn listen(rest: &[String]) -> Result<(), Box<dyn Error>> {
        let port: u16 = rest.first().ok_or("listen needs a port")?.parse()?;
        let listener = TcpListener::bind(("127.0.0.1", port))?;
        say(&format!("listening on 127.0.0.1:{port}; stop with Ctrl+C"));
        for stream in listener.incoming() {
            drop(stream);
        }
        Ok(())
    }

    /// A proxy allowing `example.com`, and its agent credential (`base64("clauseo:<token>")`).
    fn proxy() -> Result<(ProxyHandle, String), Box<dyn Error>> {
        let handle = start_proxy(ProxyConfig {
            allow: Allowlist::exact("example.com")?,
            silent: Allowlist::default(),
            upstream: Upstream::System,
            denials: Arc::new(Quiet),
            record: None,
        })?;
        let token = handle
            .env()
            .into_iter()
            .find(|(name, _)| name == "HTTPS_PROXY")
            .and_then(|(_, value)| value?.into_string().ok())
            .and_then(|value| url::Url::parse(&value).ok()?.password().map(str::to_owned))
            .ok_or("no token in the proxy's env")?;
        let credential =
            base64::engine::general_purpose::STANDARD.encode(format!("clauseo:{token}"));
        Ok((handle, credential))
    }

    pub(super) fn rows(exe: &Path, rest: &[String]) -> Result<(), Box<dyn Error>> {
        let (mut foreign, mut share) = (None, None);
        for pair in rest.chunks(2) {
            match pair {
                [flag, port] if flag == "--foreign" => foreign = Some(port.parse::<u16>()?),
                [flag, path] if flag == "--share" => share = Some(path.clone()),
                _ => return Err("usage: rows [--foreign <port>] [--share <unc path>]".into()),
            }
        }
        let pool = sandbox_pool_sids()?;
        let [a, b, ..] = pool.as_slice() else {
            return Err("the machine record lists fewer than two pool users".into());
        };
        let data_dir = sandbox_data_dir()?.ok_or("the machine record names no data folder")?;
        let (proxy_a, cred_a) = proxy()?;
        let (proxy_b, cred_b) = proxy()?;
        let relay = ports::bind_loopback()?;
        let unlisted = ports::bind_loopback()?;
        let pair_a = ports::claim_port()?;
        let pair_x = ports::claim_port()?;
        let (pa, pb) = (proxy_a.port(), proxy_b.port());
        let ra = relay.local_addr()?.port();
        let xa = unlisted.local_addr()?.port();
        let ns = Namespace::production();
        let permit_a = LaunchPermit::open_ports(&ns, a, &[pa, ra, pair_a.port()])?;
        let permit_b = LaunchPermit::open_ports(&ns, b, &[pb])?;
        // The pair ports are claims; the probe binds them itself.
        let (pair_a, pair_x) = (pair_a.port(), pair_x.port());
        let ports = Ports {
            pa,
            pb,
            ra,
            xa,
            pair_a,
            pair_x,
            browser_a: proxy_a.browser_port(),
        };
        let (mut a_rows, b_rows) = matter_rows(&ports, &cred_a, &cred_b, foreign);
        let localhost = r"share:\\localhost\C$".to_owned();
        a_rows.push(row(
            "A",
            r"the (B) token against \\localhost\C$",
            localhost,
            Blocked,
        ));
        if let Some(path) = share {
            a_rows.push(row("A", "a guest share", format!("share:{path}"), Blocked));
        }
        let local = |port: u16| format!("connect:127.0.0.1:{port}");
        let mut passed = 0;
        let mut total = 0;
        for (sid, rows) in [(a, &a_rows), (b, &b_rows)] {
            let (p, t) = run_rows(exe, &data_dir, sid, rows);
            passed += p;
            total += t;
        }
        permit_a.close();
        let after = vec![row(
            "A",
            "A's proxy port after its session ends",
            local(pa),
            Blocked,
        )];
        let (p, t) = run_rows(exe, &data_dir, a, &after);
        passed += p;
        total += t;
        permit_b.close();
        drop((proxy_a, proxy_b, relay, unlisted));
        say(&serde_json::json!({ "passed": passed, "of": total }).to_string());
        Ok(())
    }

    /// The ports the rows name.
    struct Ports {
        pa: u16,
        pb: u16,
        ra: u16,
        xa: u16,
        pair_a: u16,
        pair_x: u16,
        browser_a: u16,
    }

    /// The rows for matters A and B while both permits are open.
    fn matter_rows(
        ports: &Ports,
        cred_a: &str,
        cred_b: &str,
        foreign: Option<u16>,
    ) -> (Vec<Row>, Vec<Row>) {
        let Ports {
            pa,
            pb,
            ra,
            xa,
            pair_a,
            pair_x,
            browser_a,
        } = *ports;
        let local = |port: u16| format!("connect:127.0.0.1:{port}");
        let mut a_rows = vec![
            row(
                "A",
                "curl out direct (HTTP to example.com)",
                "http:example.com".into(),
                Blocked,
            ),
            row(
                "A",
                "a direct TCP connect by address",
                "connect:1.1.1.1:443".into(),
                Blocked,
            ),
            row(
                "A",
                "direct DNS to a public resolver",
                "udp53:1.1.1.1".into(),
                Blocked,
            ),
            row(
                "A",
                "a name lookup through Dnscache (residual)",
                "resolve:example.com".into(),
                Measured,
            ),
            row(
                "A",
                "the proxy port",
                format!("proxy:{pa}:{cred_a}"),
                Reached,
            ),
            row(
                "A",
                "a listed loopback port (the relay)",
                local(ra),
                Reached,
            ),
            row("A", "matter B's proxy port", local(pb), Blocked),
            row(
                "A",
                "the proxy's browser listener",
                local(browser_a),
                Blocked,
            ),
            row(
                "A",
                "a loopback pair on a listed port",
                format!("pair:{pair_a}"),
                Reached,
            ),
            row(
                "A",
                "a loopback pair on an unlisted port",
                format!("pair:{pair_x}"),
                Blocked,
            ),
            row("A", "an unlisted listener in the range", local(xa), Blocked),
        ];
        if let Some(port) = foreign {
            a_rows.push(row(
                "A",
                "another Windows user's listener (two-user row)",
                local(port),
                Blocked,
            ));
        }
        let b_rows = vec![
            row("B", "matter B to matter A's relay port", local(ra), Blocked),
            row("B", "matter B to matter A's proxy port", local(pa), Blocked),
            row(
                "B",
                "matter B's own proxy port",
                format!("proxy:{pb}:{cred_b}"),
                Reached,
            ),
        ];
        (a_rows, b_rows)
    }

    /// Runs `rows` as `sid` in one probe; prints each; (passed, counted).
    fn run_rows(exe: &Path, data_dir: &Path, sid: &Sid, rows: &[Row]) -> (usize, usize) {
        let steps: Vec<String> = rows.iter().map(|r| r.step.clone()).collect();
        let code = launch::as_pool_user(exe, data_dir, sid, &steps);
        let (mut passed, mut counted) = (0, 0);
        for (i, row) in rows.iter().enumerate() {
            let got = match &code {
                Ok(code) => match probe::bit(*code, i) {
                    Some(true) => "reached".to_owned(),
                    Some(false) => "blocked".to_owned(),
                    None => format!("no result: the probe exited {code:#x}"),
                },
                Err(why) => format!("no result: {why}"),
            };
            let pass = match row.expect {
                Expect::Reached => Some(got == "reached"),
                Expect::Blocked => Some(got == "blocked"),
                Expect::Measured => None,
            };
            if let Some(pass) = pass {
                counted += 1;
                passed += usize::from(pass);
            }
            let expect = match row.expect {
                Expect::Reached => "reached",
                Expect::Blocked => "blocked",
                Expect::Measured => "measured",
            };
            // The proxy step's credential stays out of the output.
            let keep = if row.step.starts_with("proxy:") { 2 } else { 4 };
            let step = row.step.split(':').take(keep).collect::<Vec<_>>().join(":");
            say(&serde_json::json!({
                "user": row.user, "pool_user": sid.as_str(), "row": row.what,
                "step": step, "expect": expect, "got": got, "pass": pass,
            })
            .to_string());
        }
        (passed, counted)
    }
}
