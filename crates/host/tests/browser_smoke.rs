//! The browser server's smoke test, run by hand: the real `clauseo-mcp browser-guard` in front of
//! agent-browser v0.41.1 and a real Chrome, started as the host's browser entry starts it
//! (`crates/profile/src/servers.rs`, fn `browser`; decision record 0029). It checks that every
//! request Chrome makes goes through the session's proxy, loopback included; that WebRTC leaks
//! no UDP `host` or `srflx` candidate; and that Chrome's temp profile is gone after the close.
//!
//! The test stands in for the host's egress proxy with a logging proxy on `127.0.0.1` that
//! tunnels `CONNECT` and forwards absolute-form `http://` requests. `smoke.test` resolves to a
//! local page server, and `127.0.0.1:<pixel port>` reaches a pixel server only through the proxy
//! (the stage C proxy refuses loopback; this one is test-only).
//!
//! By hand, on a Mac and on Windows:
//!
//! 1. Build `clauseo-mcp` from branch `s27-mcp-guard`: `cargo build -p clauseo-mcp --release`.
//! 2. Get agent-browser v0.41.1 (`npm i -g agent-browser@0.41.1`, or the release binary from
//!    <https://github.com/vercel-labs/agent-browser/releases/tag/v0.41.1>) and Chrome.
//! 3. Set `CLAUSEO_SMOKE_MCP` to the `clauseo-mcp` binary, `CLAUSEO_SMOKE_AGENT_BROWSER` to the
//!    agent-browser native binary and `CLAUSEO_SMOKE_CHROME` to the Chrome executable, each an
//!    absolute path.
//! 4. Run `cargo nextest run -p clauseo-host --run-ignored only -E 'test(browser_smoke)'`.
//!
//! The test needs the network (it opens `https://example.com`). On failure it keeps its folder
//! and prints the proxy log.

#[cfg(test)]
#[path = "browser_smoke/http.rs"]
mod http;
#[cfg(test)]
#[path = "browser_smoke/mcp.rs"]
mod mcp;
#[cfg(test)]
#[path = "browser_smoke/pages.rs"]
mod pages;
#[cfg(test)]
#[path = "browser_smoke/scratch.rs"]
mod scratch;

/// The test and its MCP steps.
#[cfg(test)]
mod smoke {
    use std::time::Duration;

    use serde_json::{Value, json};

    use super::mcp::{self, Guard, poll};
    use super::pages::World;
    use super::scratch::{self, Scratch, env_file, profiles};

    /// The UDP `host` and `srflx` candidates in the report: the ones that would leak an address.
    fn leaking(candidates: &[String]) -> Vec<&String> {
        candidates
            .iter()
            .filter(|candidate| {
                let lower = format!(" {} ", candidate.to_ascii_lowercase());
                lower.contains(" udp ")
                    && (lower.contains(" typ host ") || lower.contains(" typ srflx "))
            })
            .collect()
    }

    /// `initialize`, `notifications/initialized` and `tools/list` (every page).
    fn handshake(guard: &mut Guard) {
        let limit = Duration::from_secs(20);
        let init = json!({
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": {"name": "clauseo-browser-smoke", "version": "0"},
        });
        let result = guard.request("initialize", &init, limit);
        assert_eq!(result["serverInfo"]["name"], "agent-browser", "{result}");
        guard.notify("notifications/initialized");
        let mut names = Vec::new();
        let mut params = json!({});
        loop {
            let page = guard.request("tools/list", &params, limit);
            let tools = page["tools"].as_array().cloned().unwrap_or_default();
            names.extend(
                tools
                    .iter()
                    .filter_map(|t| t["name"].as_str().map(str::to_owned)),
            );
            let Some(cursor) = page["nextCursor"].as_str() else {
                break;
            };
            params = json!({"cursor": cursor});
        }
        for tool in [
            "agent_browser_open",
            "agent_browser_snapshot",
            "agent_browser_close",
        ] {
            assert!(
                names.iter().any(|name| name == tool),
                "no {tool} in {names:?}"
            );
        }
    }

    #[test]
    #[ignore = "by hand: needs clauseo-mcp, agent-browser v0.41.1 and Chrome; see docs/decisions/0029-browser.md"]
    fn browser_smoke() {
        let mcp = env_file("CLAUSEO_SMOKE_MCP");
        let agent_browser = env_file("CLAUSEO_SMOKE_AGENT_BROWSER");
        let chrome = env_file("CLAUSEO_SMOKE_CHROME");
        let scratch = Scratch::new();
        let cwd = scratch.cwd();
        println!("test folder: {}", scratch.root.display());
        let world = World::start();
        let mut guard = Guard::start(mcp::guard_command(
            &mcp,
            &agent_browser,
            &chrome,
            &scratch,
            world.proxy_port,
        ));
        handshake(&mut guard);

        let open = Duration::from_secs(60);
        guard.call(
            "agent_browser_open",
            &json!({"url": "https://example.com"}),
            open,
        );
        let snapshot = guard.call(
            "agent_browser_snapshot",
            &json!({"interactive": false}),
            open,
        );
        assert!(
            snapshot.to_string().contains("Example Domain"),
            "snapshot: {snapshot}"
        );
        assert!(
            !profiles(&cwd).is_empty(),
            "no {} folder under {} while Chrome runs: TMPDIR/TEMP did not reach agent-browser",
            scratch::PROFILE_PREFIX,
            cwd.display()
        );
        let page_a = format!("http://smoke.test:{}", world.port_a);
        guard.call(
            "agent_browser_open",
            &json!({"url": format!("{page_a}/")}),
            open,
        );
        guard.call(
            "agent_browser_open",
            &json!({"url": format!("{page_a}/webrtc")}),
            open,
        );
        let reported = poll(Duration::from_secs(15), || {
            world.report.lock().expect("report").is_some()
        });
        poll(Duration::from_secs(5), || {
            !world.pixel.lock().expect("pixel").is_empty()
        });
        guard.call("agent_browser_close", &json!({}), open);
        let status = guard.finish(Duration::from_secs(30));
        println!("guard exited: {status}");

        let log = world.log();
        println!("proxy log: {log:?}");
        let allowed = [
            "example.com:443".to_owned(),
            format!("smoke.test:{}", world.port_a),
            format!("127.0.0.1:{}", world.port_b),
        ];
        let others: Vec<&String> = log.iter().filter(|t| !allowed.contains(t)).collect();
        assert!(
            others.is_empty(),
            "unexpected hosts {others:?}; proxy log: {log:?}"
        );
        for target in &allowed {
            assert!(
                log.contains(target),
                "{target} never reached the proxy; proxy log: {log:?}"
            );
        }
        let pixel = world.pixel.lock().expect("pixel").clone();
        assert!(
            !pixel.is_empty() && pixel.iter().all(|&via| via),
            "the pixel must be fetched, and only through the proxy: {pixel:?}; proxy log: {log:?}"
        );

        assert!(reported, "no WebRTC report within 15 s; proxy log: {log:?}");
        let body = world
            .report
            .lock()
            .expect("report")
            .clone()
            .unwrap_or_default();
        let candidates: Vec<String> = serde_json::from_str::<Value>(&body)
            .ok()
            .and_then(|value| serde_json::from_value(value).ok())
            .unwrap_or_else(|| panic!("report is not a list of strings: {body}"));
        println!("candidates: {candidates:?}");
        assert!(
            !candidates.iter().any(|c| c.starts_with("error:")),
            "the WebRTC page failed: {candidates:?}; proxy log: {log:?}"
        );
        let leaks = leaking(&candidates);
        assert!(
            leaks.is_empty(),
            "UDP host/srflx candidates {leaks:?}; proxy log: {log:?}"
        );

        let gone = poll(Duration::from_secs(10), || profiles(&cwd).is_empty());
        assert!(
            gone,
            "temp profiles left after close: {:?}; proxy log: {log:?}",
            profiles(&cwd)
        );
        scratch.remove();
    }
}
