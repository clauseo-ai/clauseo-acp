//! A certificate warning has no way past it through the guard: the real guard, agent-browser and
//! Chrome, against two pages whose certificates do not verify, each served by a local
//! `openssl s_server -WWW` that the test proxy tunnels `CONNECT <host>:443` to
//! (`page_proxy.rs`). `bad-self.test` has a self-signed certificate for its own name;
//! `bad-name.test` has one for `other.test` (self-signed too, so that page fails the name check
//! and the trust check at once: a wrong-name-only page needs a CA the browser trusts).
//! Certificates are made at test time with the `openssl` command; no crate is added
//! (`deps.rs` bans rustls).
//!
//! For each page, every route the agent has is tried: open, click "Advanced" and then
//! "Proceed", type `thisisunsafe` (as keys and as typed text), reload, back and forward,
//! `tab_new`, and (for `bad-self.test`) an `http://` page that redirects to it. After each, the
//! page's own [`MARKER`] is in neither the snapshot nor the text, and the warning is. A control
//! fetches [`MARKER`] through the same proxy and tunnel with `openssl s_client`, so the rows
//! cannot pass because the page was never servable. What stops it is the profile's `ssl`
//! preferences (`src/browser_guard/quiet.rs`): with them, Chrome for Testing 147 builds the
//! warning with no "Proceed" and ignores the typed phrase; without them the phrase loads the page.
//!
//! [`inside_routes_to_the_setting_are_closed`] tries the routes to the setting itself: Chrome's
//! and Edge's own pages (refused by the guard's URL check, `filter.rs` rule 5e, when opened;
//! refused by Chrome when a page links to them) and launch switches passed as tool arguments
//! (refused by `filter.rs` rules 5a, 5f, 5g and 5h; Chrome's arguments are the guard's own,
//! `chrome.rs` `chrome_args`).
//!
//! Without agent-browser these tests skip, as the others do; with it, a missing `openssl` fails
//! them, naming what is missing.

use std::collections::BTreeMap;
use std::io::Write as _;
use std::net::TcpStream;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::{binaries, command, handshake};
use crate::page_proxy::{HEADING, INTERNAL, INTERNAL_LINKS, PageProxy, REDIRECT};
use crate::support::{Guard, Scratch};
use crate::write;

/// The real page's text: it must never reach the agent.
const MARKER: &str = "CLAUSEO-CERT-PAGE-MARKER-5e1d";
/// The warning's heading, which the agent sees instead (beside `NET::ERR_CERT_*`, and
/// `Navigation failed: net::ERR_CERT_*` as `open`'s answer).
const WARNING: &str = "Your connection is not private";
/// The origin of Chrome's error page, the certificate warning's.
const ERROR_PAGE: &str = "chrome-error://chromewebdata/";
/// The pages: host, and the name its certificate gives.
const PAGES: [(&str, &str); 2] = [
    ("bad-self.test", "bad-self.test"),
    ("bad-name.test", "other.test"),
];

/// The TLS servers, killed when dropped.
struct TlsPages {
    servers: Vec<Child>,
    /// `host:443` and the local port its server listens on.
    tunnels: BTreeMap<String, u16>,
}

impl Drop for TlsPages {
    fn drop(&mut self) {
        for server in &mut self.servers {
            drop(server.kill());
            drop(server.wait());
        }
    }
}

/// Whether `openssl` runs here.
fn has_openssl() -> bool {
    Command::new("openssl")
        .arg("version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// Runs `openssl` with `args` in `dir`; fails the test with its output if it fails.
fn openssl(dir: &Path, args: &[&str]) {
    let output = Command::new("openssl")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "openssl {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A certificate for each of [`PAGES`] and an `openssl s_server -WWW` serving [`MARKER`] with it.
/// Called once agent-browser is known to be here; fails, naming it, when `openssl` is not.
fn tls_pages(scratch: &Scratch) -> TlsPages {
    assert!(
        has_openssl(),
        "the certificate rows need the openssl command, which is not on PATH: install OpenSSL \
         (the test makes its certificates and serves its pages with it)"
    );
    let mut pages = TlsPages {
        servers: Vec::new(),
        tunnels: BTreeMap::new(),
    };
    for (host, name) in PAGES {
        let dir = scratch.path().join(format!("tls-{host}"));
        write::create_dir_all(&dir).unwrap();
        openssl(
            &dir,
            &[
                "req",
                "-x509",
                "-newkey",
                "rsa:2048",
                "-nodes",
                "-keyout",
                "key.pem",
                "-out",
                "cert.pem",
                "-days",
                "2",
                "-subj",
                &format!("/CN={name}"),
                "-addext",
                &format!("subjectAltName=DNS:{name}"),
            ],
        );
        write::write(
            dir.join("index.html"),
            format!(
                "<!doctype html><html><head><title>Real page</title></head><body><h1>{MARKER}\
                 </h1><p>The page behind the certificate warning.</p></body></html>"
            ),
        )
        .unwrap();
        let port = super::free_port();
        let server = Command::new("openssl")
            .args(["s_server", "-quiet", "-WWW", "-accept"])
            .arg(format!("127.0.0.1:{port}"))
            .args(["-cert", "cert.pem", "-key", "key.pem"])
            .current_dir(&dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        pages.servers.push(server);
        let port: u16 = port.parse().unwrap();
        let started = Instant::now();
        while TcpStream::connect(("127.0.0.1", port)).is_err() {
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "openssl s_server for {host} did not listen on {port}"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
        pages.tunnels.insert(format!("{host}:443"), port);
    }
    pages
}

/// The control: `host`'s page, fetched through the proxy's tunnel with `openssl s_client`
/// (which goes on past a failed verification), holds [`MARKER`].
fn assert_servable(proxy: &PageProxy, host: &str) {
    let mut client = Command::new("openssl")
        .args(["s_client", "-quiet", "-proxy"])
        .arg(format!("127.0.0.1:{}", proxy.port))
        .arg("-connect")
        .arg(format!("{host}:443"))
        .arg("-servername")
        .arg(host)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = client.stdin.take().unwrap();
    stdin
        .write_all(b"GET /index.html HTTP/1.0\r\n\r\n")
        .unwrap();
    // s_server -WWW closes the connection after its answer, which ends s_client.
    let output = client.wait_with_output().unwrap();
    drop(stdin);
    let body = String::from_utf8_lossy(&output.stdout);
    assert!(
        body.contains(MARKER),
        "the control could not fetch {host}'s page through the proxy: {body}"
    );
}

/// The guard and its last request id.
struct Agent {
    guard: Guard,
    id: u64,
}

impl Agent {
    /// A tool call's whole reply, a refusal included (`isError`); fails only on a JSON-RPC error.
    fn call(&mut self, tool: &str, arguments: &Value) -> Value {
        self.id += 1;
        let id = self.id;
        self.guard
            .send(&json!({"jsonrpc": "2.0", "id": id, "method": "tools/call",
            "params": {"name": tool, "arguments": arguments}}));
        loop {
            let reply = self.guard.read();
            if reply["id"] != json!(id) {
                continue;
            }
            assert!(reply.get("error").is_none(), "{tool}: {reply}");
            let result = reply["result"].clone();
            eprintln!(
                "{tool} {arguments}: {}",
                if result["isError"] == json!(true) {
                    "error"
                } else {
                    "ok"
                }
            );
            return result;
        }
    }

    /// What the agent can read of the page: its snapshot and its text.
    fn seen(&mut self) -> String {
        let snapshot = self.call("agent_browser_snapshot", &json!({}));
        let text = self.call("agent_browser_get_text", &json!({"selector": "body"}));
        format!("{snapshot}\n{text}")
    }

    /// After `row`, the warning shows and the page does not; with no "Proceed" anywhere.
    fn assert_warning(&mut self, row: &str) -> String {
        // The navigation or the typed keys have time to land.
        self.call("agent_browser_wait_ms", &json!({"ms": 1500}));
        let seen = self.seen();
        assert!(
            !seen.contains(MARKER),
            "{row}: the page's content showed: {seen}"
        );
        // The tab is on Chrome's error page: its heading, or its origin (once "Advanced" is open
        // the heading and the error code are hidden; agent-browser names the origin either way).
        assert!(
            seen.contains(WARNING) || seen.contains(ERROR_PAGE),
            "{row}: no certificate warning: {seen}"
        );
        assert!(!seen.contains("Proceed to"), "{row}: a way past: {seen}");
        seen
    }
}

fn start(
    scratch: &Scratch,
    binaries: &(std::path::PathBuf, std::path::PathBuf),
    proxy: &PageProxy,
) -> Agent {
    let mut guard = Guard::start(command(scratch, binaries, proxy));
    handshake(&mut guard);
    Agent { guard, id: 10 }
}

/// Every row for one page; `redirect` adds the `http://` redirect row (to `bad-self.test`).
fn rows(host: &str, redirect: bool) {
    let Some(binaries) = binaries() else { return };
    let scratch = Scratch::new(&format!("real_cert_{}", host.replace('.', "_")));
    let pages = tls_pages(&scratch);
    let proxy = PageProxy::with_tunnels(pages.tunnels.clone());
    assert_servable(&proxy, host);
    let url = format!("https://{host}/index.html");
    let mut agent = start(&scratch, &binaries, &proxy);

    let opened = agent.call("agent_browser_open", &json!({"url": url}));
    let seen = agent.assert_warning("open");
    // The text the agent sees, for the pull request.
    eprintln!("OPEN'S ANSWER ({host}): {opened}");
    eprintln!("WHAT THE AGENT SEES ({host}): {seen}");

    // "Advanced", then "Proceed", by the warning's own ids. With the setting, Chrome leaves the
    // proceed link out of the page ("Element not found"); without it, this click loads the
    // page (checked by hand against Chrome for Testing 147, PR text).
    agent.call(
        "agent_browser_click",
        &json!({"selector": "#details-button"}),
    );
    let advanced = agent.assert_warning("Advanced");
    eprintln!("AFTER ADVANCED ({host}): {advanced}");
    agent.call("agent_browser_click", &json!({"selector": "#proceed-link"}));
    agent.assert_warning("Proceed");

    // `thisisunsafe`, key by key and as typed text.
    for key in "thisisunsafe".chars() {
        agent.call("agent_browser_press", &json!({"key": key.to_string()}));
    }
    agent.assert_warning("thisisunsafe pressed");
    agent.call(
        "agent_browser_type",
        &json!({"selector": "body", "text": "thisisunsafe"}),
    );
    agent.assert_warning("thisisunsafe typed");

    agent.call("agent_browser_reload", &json!({}));
    agent.assert_warning("reload");

    agent.call("agent_browser_open", &json!({"url": "http://page.test/"}));
    agent.call("agent_browser_back", &json!({}));
    agent.assert_warning("back");
    agent.call("agent_browser_forward", &json!({}));
    agent.call("agent_browser_back", &json!({}));
    agent.assert_warning("forward, then back");

    agent.call("agent_browser_tab_new", &json!({"url": url}));
    agent.assert_warning("tab_new");

    if redirect {
        agent.call(
            "agent_browser_open",
            &json!({"url": format!("http://page.test{REDIRECT}")}),
        );
        agent.assert_warning("http to https redirect");
    }

    agent.call("agent_browser_close", &json!({}));
    let (status, _) = agent.guard.finish();
    assert_eq!(status.code(), Some(0));
    // Chrome reached the page's server: the warning was its certificate's, not a refused proxy.
    let connect = format!("CONNECT {host}:443");
    assert!(
        proxy.log().iter().any(|line| line == &connect),
        "{:?}",
        proxy.log()
    );
    drop(pages);
}

#[test]
fn a_self_signed_page_has_no_way_past_its_warning() {
    rows("bad-self.test", true);
}

#[test]
fn a_wrong_name_page_has_no_way_past_its_warning() {
    rows("bad-name.test", false);
}

/// The routes to the setting itself: Chrome's and Edge's own pages, opened and linked to, and
/// launch switches as tool arguments. Each is refused, and the warning stays.
#[test]
fn inside_routes_to_the_setting_are_closed() {
    let Some(binaries) = binaries() else { return };
    let scratch = Scratch::new("real_cert_inside");
    let pages = tls_pages(&scratch);
    let proxy = PageProxy::with_tunnels(pages.tunnels.clone());
    let mut agent = start(&scratch, &binaries, &proxy);
    let refused = |result: &Value, row: &str| {
        assert_eq!(result["isError"], json!(true), "{row}: {result}");
        let text = result.to_string();
        assert!(
            text.contains("Clauseo does not allow") || text.contains("not supported in Clauseo"),
            "{row}: {text}"
        );
    };

    // Opened by the agent: refused by the guard's URL check.
    for url in INTERNAL_LINKS {
        let result = agent.call("agent_browser_open", &json!({"url": url}));
        refused(&result, &format!("open {url}"));
        let result = agent.call("agent_browser_tab_new", &json!({"url": url}));
        refused(&result, &format!("tab_new {url}"));
    }

    // Linked from a web page: Chrome refuses a web page's navigation to its own pages.
    agent.call(
        "agent_browser_open",
        &json!({"url": format!("http://page.test{INTERNAL}")}),
    );
    for (n, url) in INTERNAL_LINKS.iter().enumerate() {
        agent.call(
            "agent_browser_click",
            &json!({"selector": format!("#i{n}")}),
        );
        agent.call("agent_browser_wait_ms", &json!({"ms": 500}));
        let at = agent.call("agent_browser_get_url", &json!({}));
        assert!(
            at.to_string()
                .contains(&format!("http://page.test{INTERNAL}")),
            "a click on {url} left the page: {at}"
        );
        let seen = agent.seen();
        assert!(seen.contains(HEADING), "a click on {url}: {seen}");
    }

    // Launch switches, as each way a tool takes arguments.
    for switch in [
        "--ignore-certificate-errors",
        "--ignore-certificate-errors-spki-list=AAAA",
        "--allow-insecure-localhost",
        "--test-type",
        "--user-data-dir=/tmp/other",
    ] {
        let tries = [
            json!({"url": switch}),
            json!({"url": "https://bad-self.test/index.html", "args": [switch]}),
            json!({"url": "https://bad-self.test/index.html", "extraArgs": [switch]}),
        ];
        for arguments in tries {
            let result = agent.call("agent_browser_open", &arguments);
            refused(&result, &format!("open {arguments}"));
        }
    }
    for arguments in [
        json!({"url": "https://bad-self.test/index.html", "ignoreHttpsErrors": true}),
        json!({"url": "https://bad-self.test/index.html", "caCert": "cert.pem"}),
        json!({"url": "https://bad-self.test/index.html", "useSystemCa": true}),
    ] {
        let result = agent.call("agent_browser_open", &arguments);
        refused(&result, &format!("open {arguments}"));
    }

    // After all of it, the page still has no way past its warning.
    agent.call(
        "agent_browser_open",
        &json!({"url": "https://bad-self.test/index.html"}),
    );
    agent.assert_warning("after the inside routes");
    agent.call("agent_browser_close", &json!({}));
    let (status, _) = agent.guard.finish();
    assert_eq!(status.code(), Some(0));
    drop(pages);
}
