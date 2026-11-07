//! The rows (S15 PR7, orchestrator ruling 2026-11-07): set-up's blocks catch every pool token,
//! derived ones included, everywhere outside 127.0.0.1:46100-46199; inside the range a static
//! permit lets every pool user reach every listener, and the listeners' per-launch secrets keep
//! matters apart (those rows are `measured`); the owner may read set-up's filters and change
//! none; a launch refuses a filter set-up didn't write and a listener in the range that isn't
//! this app's (`foreign`).

use std::error::Error;
use std::net::TcpListener;
use std::path::Path;
use std::sync::Arc;

use base64::Engine;
use clauseo_sandbox::windows::sids::Sid;
use clauseo_sandbox::windows::wfp::{
    Namespace, check_launch_in, foreign_test_filter, probe_owner_grants,
};
use clauseo_sandbox::windows::{sandbox_data_dir, sandbox_pool_sids};
use clauseo_sandbox::{
    Allowlist, Denial, DenialSink, ProxyConfig, ProxyHandle, Reachability, Upstream, ports,
    start_proxy,
};

use super::probe::{Got, MAX_STEPS, step};
use super::{launch, say};

/// Denials go nowhere: the rows say what was reached.
struct Quiet;

impl DenialSink for Quiet {
    fn denied(&self, _denial: Denial) {}
    fn upstream(&self, _reach: Reachability) {}
}

/// What a row expects of its step.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Expect {
    /// It gets through.
    Reached,
    /// A filter refuses it (`WSAEACCES`).
    Filtered,
    /// It doesn't get through, filtered or otherwise.
    NotReached,
    /// Measured and reported, neither passing nor failing (a residual or a range row).
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

/// A launch's check for each recorded pool user.
pub(crate) fn status() -> Result<(), Box<dyn Error>> {
    for sid in sandbox_pool_sids()? {
        let line = match check_launch_in(&Namespace::production(), &sid, &[]) {
            Ok(()) => serde_json::json!({ "pool_user": sid.as_str(), "filtered": true }),
            Err(error) => serde_json::json!({
                "pool_user": sid.as_str(), "filtered": false, "why": error.to_string(),
            }),
        };
        say(&line.to_string());
    }
    Ok(())
}

/// `foreign check`, as the desktop user: a launch's check must refuse, while another Windows
/// user runs `listen <port in 46100-46199>` (a listener that isn't this app's) or after
/// `foreign add` (a filter set-up didn't write; does the owner's enumeration see it?).
/// `foreign add` and `foreign remove`, elevated: that filter, persistent, in our sublayer.
pub(crate) fn foreign(rest: &[String]) -> Result<(), Box<dyn Error>> {
    let ns = Namespace::production();
    match rest.first().map(String::as_str) {
        Some("add") => {
            foreign_test_filter(&ns, false)?;
            say("added; run `foreign check` as the desktop user, then `foreign remove` here");
        }
        Some("remove") => {
            foreign_test_filter(&ns, true)?;
            say("removed");
        }
        Some("check") => {
            let pool = sandbox_pool_sids()?;
            let a = pool
                .first()
                .ok_or("the machine record lists no pool user")?;
            let checked = check_launch_in(&ns, a, &[]);
            let got = match &checked {
                Ok(()) => "allowed".to_owned(),
                Err(error) => format!("refused ({error})"),
            };
            let what = "a launch with a foreign filter or listener present";
            report(
                "A",
                a.as_str(),
                what,
                "check",
                Expect::NotReached,
                &got,
                Some(checked.is_err()),
            );
        }
        _ => return Err("usage: foreign add | check | remove".into()),
    }
    Ok(())
}

/// A listener on `127.0.0.1:<port>` that accepts until killed.
pub(crate) fn listen(rest: &[String]) -> Result<(), Box<dyn Error>> {
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
        // Tier B opens no browser listener (D10).
        browser: false,
        listener: ports::ListenerFor::TierB,
    })?;
    let token = handle
        .env()
        .into_iter()
        .find(|(name, _)| name == "HTTPS_PROXY")
        .and_then(|(_, value)| value?.into_string().ok())
        .and_then(|value| url::Url::parse(&value).ok()?.password().map(str::to_owned))
        .ok_or("no token in the proxy's env")?;
    let credential = base64::engine::general_purpose::STANDARD.encode(format!("clauseo:{token}"));
    Ok((handle, credential))
}

/// The optional rows' arguments.
#[derive(Default)]
struct Options {
    share: Option<String>,
    inbound: Option<u16>,
    smb_host: Option<String>,
    webdav_host: Option<String>,
}

fn options(rest: &[String]) -> Result<Options, Box<dyn Error>> {
    let mut options = Options::default();
    for pair in rest.chunks(2) {
        match pair {
            [flag, value] if flag == "--share" => options.share = Some(value.clone()),
            [flag, value] if flag == "--inbound" => options.inbound = Some(value.parse()?),
            [flag, value] if flag == "--smb-host" => options.smb_host = Some(value.clone()),
            [flag, value] if flag == "--webdav-host" => options.webdav_host = Some(value.clone()),
            _ => return Err(format!("unknown arguments {pair:?}").into()),
        }
    }
    Ok(options)
}

/// The ports the rows name.
struct Ports {
    pa: u16,
    pb: u16,
    ra: u16,
    xa: u16,
    pair_a: u16,
    squat: u16,
}

pub(crate) fn rows(exe: &Path, rest: &[String]) -> Result<(), Box<dyn Error>> {
    let options = options(rest)?;
    let pool = sandbox_pool_sids()?;
    let [a, b, ..] = pool.as_slice() else {
        return Err("the machine record lists fewer than two pool users".into());
    };
    let data_dir = sandbox_data_dir()?.ok_or("the machine record names no data folder")?;
    let (proxy_a, cred_a) = proxy()?;
    let (proxy_b, cred_b) = proxy()?;
    let relay = ports::bind_loopback(ports::ListenerFor::TierB)?;
    let unlisted = ports::bind_loopback(ports::ListenerFor::TierB)?;
    let ports = Ports {
        pa: proxy_a.port(),
        pb: proxy_b.port(),
        ra: relay.local_addr()?.port(),
        xa: unlisted.local_addr()?.port(),
        pair_a: ports::claim_port()?.port(),
        squat: ports::claim_port()?.port(),
    };
    let ns = Namespace::production();
    check_launch_in(&ns, a, &[ports.pa, ports.ra, ports.pair_a, ports.squat])?;
    check_launch_in(&ns, b, &[ports.pb])?;
    if let Some(port) = options.inbound {
        say(&format!(
            "connect from another machine to this machine's IP, port {port}, within 30 s"
        ));
    }
    let mut tally = (0, 0);
    let a_rows = matter_a_rows(&ports, &cred_a, &cred_b, &options);
    let b_rows = matter_b_rows(&ports, &cred_b);
    for (sid, list) in [(a, &a_rows), (b, &b_rows)] {
        for chunk in list.chunks(MAX_STEPS) {
            add(&mut tally, run_rows(exe, &data_dir, sid, chunk));
        }
    }
    add(&mut tally, squat_row(exe, &data_dir, a, b, ports.squat));
    add(&mut tally, owner_rows(&ns));
    drop((proxy_a, proxy_b, relay, unlisted));
    say(&serde_json::json!({ "passed": tally.0, "of": tally.1 }).to_string());
    Ok(())
}

fn add(tally: &mut (usize, usize), more: (usize, usize)) {
    tally.0 += more.0;
    tally.1 += more.1;
}

/// Matter A's rows: outside the range, inside it, and the optional ones.
fn matter_a_rows(p: &Ports, cred_a: &str, cred_b: &str, options: &Options) -> Vec<Row> {
    let mut rows = outside_rows(p);
    rows.extend(range_rows(p, cred_a, cred_b));
    rows.extend(optional_rows(options));
    rows
}

/// A's rows outside 127.0.0.1:46100-46199, where set-up's blocks must hold.
fn outside_rows(p: &Ports) -> Vec<Row> {
    use Expect::{Filtered, Measured, NotReached};
    let s = |text: &str| text.to_owned();
    let table: Vec<(&str, String, Expect)> = vec![
        (
            "curl out direct (HTTP to example.com)",
            s("http:example.com"),
            NotReached,
        ),
        (
            "a direct TCP connect by address",
            s("connect:1.1.1.1:443"),
            Filtered,
        ),
        (
            "TCP 53 to a public resolver",
            s("connect:1.1.1.1:53"),
            Filtered,
        ),
        (
            "direct DNS (UDP 53) to a public resolver",
            s("udp53:1.1.1.1"),
            NotReached,
        ),
        ("UDP to another port", s("udp:1.1.1.1:123"), Filtered),
        ("loopback UDP", format!("udp:127.0.0.1:{}", p.pa), Filtered),
        ("ICMP echo", s("icmp:1.1.1.1"), NotReached),
        (
            "a name lookup through Dnscache (residual)",
            s("resolve:example.com"),
            Measured,
        ),
        (
            "IPv6 loopback",
            format!("connect:[::1]:{}", p.pa),
            NotReached,
        ),
        (
            "a global IPv6 address",
            s("connect:[2606:4700:4700::1111]:443"),
            NotReached,
        ),
        (
            "IPv4-mapped loopback (measured, expected reached)",
            format!("connect:[::ffff:127.0.0.1]:{}", p.pa),
            Measured,
        ),
        ("127.0.0.2", format!("connect:127.0.0.2:{}", p.pa), Filtered),
        (
            "a deny-only-user child, out",
            s("derived:denyonly:connect:1.1.1.1:443"),
            NotReached,
        ),
        (
            "a deny-only-user child, loopback outside the range",
            s("derived:denyonly:connect:127.0.0.1:445"),
            NotReached,
        ),
        (
            "an AppContainer child, out",
            s("derived:lowbox:connect:1.1.1.1:443"),
            NotReached,
        ),
        (
            "a child with its local groups deny-only (the pool group among them), out",
            s("derived:denyonlygroups:connect:1.1.1.1:443"),
            NotReached,
        ),
        (
            "a child with its user and local groups deny-only, loopback outside the range",
            s("derived:denyonlyall:connect:127.0.0.1:445"),
            NotReached,
        ),
        (
            "a deny-only-user child to A's own proxy (the permit is for the user, enabled)",
            format!("derived:denyonly:connect:127.0.0.1:{}", p.pa),
            NotReached,
        ),
        (
            "an AppContainer child of a deny-only-user token, out",
            s("derived:lowboxdenyonly:connect:1.1.1.1:443"),
            NotReached,
        ),
        (
            "a socket under anonymous impersonation, out (gets through only if \
             EveryoneIncludesAnonymous=1 and WFP reads the thread's token)",
            s("anon:1.1.1.1:443"),
            NotReached,
        ),
        (
            r"the (B) token against \\localhost\C$",
            s(r"share:\\localhost\C$"),
            NotReached,
        ),
    ];
    table
        .into_iter()
        .map(|(what, step, expect)| row("A", what, step, expect))
        .collect()
}

/// A's rows inside the range: its own listeners reached; other matters' listeners reached at
/// the WFP layer (measured), the secret refusing a wrong credential.
fn range_rows(p: &Ports, cred_a: &str, cred_b: &str) -> Vec<Row> {
    use Expect::{Measured, NotReached, Reached};
    let local = |port: u16| format!("connect:127.0.0.1:{port}");
    let range = "the range is open to every pool user; the listener's secret is the check";
    let table: [(String, String, Expect); 7] = [
        (
            "the proxy port".into(),
            format!("proxy:{}:{cred_a}", p.pa),
            Reached,
        ),
        (
            "a listed loopback port (the relay)".into(),
            local(p.ra),
            Reached,
        ),
        (
            "a loopback pair on a listed port".into(),
            format!("pair:{}", p.pair_a),
            Reached,
        ),
        (
            format!("matter B's proxy port: {range}"),
            local(p.pb),
            Measured,
        ),
        (
            "matter B's proxy with A's own credential (407)".into(),
            format!("proxy:{}:{cred_a}", p.pb),
            NotReached,
        ),
        (
            "matter B's proxy with B's credential (control)".into(),
            format!("proxy:{}:{cred_b}", p.pb),
            Measured,
        ),
        (
            format!("an unlisted listener in the range: {range}"),
            local(p.xa),
            Measured,
        ),
    ];
    table
        .into_iter()
        .map(|(what, step, expect)| row("A", &what, step, expect))
        .collect()
}

/// A's rows that need an operator's argument.
fn optional_rows(options: &Options) -> Vec<Row> {
    let mut rows = Vec::new();
    if let Some(path) = &options.share {
        rows.push(row(
            "A",
            "a guest share",
            format!("share:{path}"),
            Expect::NotReached,
        ));
    }
    if let Some(port) = options.inbound {
        let step = format!("accept:0.0.0.0:{port}:30");
        rows.push(row(
            "A",
            "inbound from another machine",
            step,
            Expect::NotReached,
        ));
    }
    if let Some(host) = &options.smb_host {
        say(&format!(
            "watch {host} for a TCP 445 connection from this machine (SMB residual)"
        ));
        let step = format!(r"share:\\{host}\x");
        rows.push(row(
            "A",
            "remote SMB (watch the host)",
            step,
            Expect::Measured,
        ));
    }
    if let Some(host) = &options.webdav_host {
        let os = std::env::var("OS").unwrap_or_default();
        say(&format!(
            "watch {host}'s HTTPS log for the path (WebDAV residual); this machine: {os}, \
             build in 'ver' (run on a Windows 11 copy with WebClient, else win-01)"
        ));
        let step = format!(r"share:\\{host}@SSL\DavWWWRoot\x");
        rows.push(row("A", "WebDAV (watch the host)", step, Expect::Measured));
    }
    rows
}

/// Matter B's rows.
fn matter_b_rows(p: &Ports, cred_b: &str) -> Vec<Row> {
    let local = |port: u16| format!("connect:127.0.0.1:{port}");
    let range = "the range is open to every pool user; the listener's secret is the check";
    vec![
        row(
            "B",
            &format!("matter B to matter A's relay port: {range}"),
            local(p.ra),
            Expect::Measured,
        ),
        row(
            "B",
            &format!("matter B to matter A's proxy port: {range}"),
            local(p.pa),
            Expect::Measured,
        ),
        row(
            "B",
            "matter B's own proxy port",
            format!("proxy:{}:{cred_b}", p.pb),
            Expect::Reached,
        ),
        row(
            "B",
            "direct out",
            "connect:1.1.1.1:443".into(),
            Expect::Filtered,
        ),
    ]
}

/// What `got` says against `expect`: `Some(pass)`, or `None` for a measured row.
fn judge(expect: Expect, got: Option<Got>) -> Option<bool> {
    match expect {
        Expect::Reached => Some(got == Some(Got::Reached)),
        Expect::Filtered => Some(got == Some(Got::Filtered)),
        Expect::NotReached => Some(matches!(got, Some(Got::Filtered | Got::Failed))),
        Expect::Measured => None,
    }
}

/// Prints one row; (passed, counted).
fn report(
    user: &str,
    pool_user: &str,
    what: &str,
    step_text: &str,
    expect: Expect,
    got: &str,
    pass: Option<bool>,
) -> (usize, usize) {
    let expect = match expect {
        Expect::Reached => "reached",
        Expect::Filtered => "filtered",
        Expect::NotReached => "not reached",
        Expect::Measured => "measured",
    };
    // The proxy step's credential stays out of the output.
    let keep = if step_text.starts_with("proxy:") {
        2
    } else {
        4
    };
    let shown = step_text
        .split(':')
        .take(keep)
        .collect::<Vec<_>>()
        .join(":");
    say(&serde_json::json!({
        "user": user, "pool_user": pool_user, "row": what,
        "step": shown, "expect": expect, "got": got, "pass": pass,
    })
    .to_string());
    pass.map_or((0, 0), |pass| (usize::from(pass), 1))
}

/// `got` in words.
fn words(code: &Result<u32, String>, i: usize) -> (Option<Got>, String) {
    match code {
        Ok(code) => match step(*code, i) {
            Some(got) => (Some(got), format!("{got:?}").to_lowercase()),
            None => (None, format!("no result: the probe exited {code:#x}")),
        },
        Err(why) => (None, format!("no result: {why}")),
    }
}

/// Runs `rows` (at most [`MAX_STEPS`]) as `sid` in one probe; prints each; (passed, counted).
fn run_rows(exe: &Path, data_dir: &Path, sid: &Sid, rows: &[Row]) -> (usize, usize) {
    let steps: Vec<String> = rows.iter().map(|r| r.step.clone()).collect();
    let code = launch::as_pool_user(exe, data_dir, sid, &steps);
    let mut tally = (0, 0);
    for (i, row) in rows.iter().enumerate() {
        let (got, text) = words(&code, i);
        let pass = judge(row.expect, got);
        add(
            &mut tally,
            report(
                row.user,
                sid.as_str(),
                &row.what,
                &row.step,
                row.expect,
                &text,
                pass,
            ),
        );
    }
    tally
}

/// The squatted-port row: B listens on one of A's listed ports nobody bound, and A connects.
/// The collusion channel 0049 records, so it is measured.
fn squat_row(exe: &Path, data_dir: &Path, a: &Sid, b: &Sid, port: u16) -> (usize, usize) {
    let listener =
        launch::start_as_pool_user(exe, data_dir, b, &[format!("accept:127.0.0.1:{port}:20")]);
    // Give B's probe five seconds to log on and bind.
    for _ in 0..50 {
        super::derived::pause_tenth();
    }
    let what = "A reaches a listener B put on one of A's unbound listed ports (collusion residual)";
    let connect = launch::as_pool_user(exe, data_dir, a, &[format!("connect:127.0.0.1:{port}")]);
    let accepted = listener.and_then(launch::Running::wait);
    let (_, a_text) = words(&connect, 0);
    let (_, b_text) = words(&accepted, 0);
    let got = format!("A: {a_text}; B's listener: {b_text}");
    report(
        "A",
        a.as_str(),
        what,
        &format!("connect:127.0.0.1:{port}"),
        Expect::Measured,
        &got,
        None,
    )
}

/// The owner's rights on set-up's WFP objects, probed in this process, unelevated.
fn owner_rows(ns: &Namespace) -> (usize, usize) {
    let mut tally = (0, 0);
    for probe in probe_owner_grants(ns) {
        let got = match &probe.result {
            Ok(()) => "allowed".to_owned(),
            Err(error) => format!("refused ({error})"),
        };
        // A refusal passes only as access denied from the call under test.
        let pass = probe.passed();
        let expect = if probe.expect_refused {
            Expect::NotReached
        } else {
            Expect::Reached
        };
        add(
            &mut tally,
            report("owner", "-", probe.what, "wfp", expect, &got, Some(pass)),
        );
    }
    tally
}
