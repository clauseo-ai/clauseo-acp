//! The session proxy's config: what it allows and refuses silently, and the denial sink, which
//! only counts.

use std::sync::Arc;

use clauseo_blocklist::{Current, Generated, Header, parse_plain};
use clauseo_protocol::AgentId;
use clauseo_sandbox::ports::{ListenerFor, TIER_B_PORTS};
use clauseo_sandbox::{
    Denial, DenialSink as _, HostRefusal, ProxyConfig, ProxyMode, Upstream, start_proxy,
};

use super::super::{RunFolders, RunSweep};
use super::{CountDenials, EgressGuard, end, proxy_config, write_secret};
use crate::actor::input::Input;
use crate::config::SessionKey;

fn agent(name: &str) -> AgentId {
    AgentId::new(name).unwrap()
}

fn config(name: &str) -> ProxyConfig {
    proxy_config(
        &agent(name),
        Arc::new(CountDenials::default()),
        (None, Arc::default()),
        true,
    )
}

/// A loaded list naming one host.
fn loaded() -> Arc<Current> {
    let header = Header {
        generated: Generated::parse("2026-11-07T06:00:00Z").unwrap(),
        entries: 1,
    };
    Arc::new(Current::new(
        parse_plain(b"evil.example p\n", header).unwrap(),
    ))
}

/// The session's proxy, open mode with the browser listener, starts with no block list loaded
/// (nothing blocked, one warning) and with one.
#[test]
fn the_session_proxy_starts_with_or_without_a_block_list() {
    let without = start_proxy(config("codex-acp"));
    assert!(without.is_ok(), "{without:?}");
    let mut with_list = config("codex-acp");
    with_list.block = loaded();
    assert!(start_proxy(with_list).is_ok());
}

/// The browser listener is asked for only by a launch that offers the browser; either way the
/// proxy runs in open mode (ADR 0053): any public host but the listed and private ones.
#[test]
fn the_browser_listener_follows_the_launch_in_open_mode() {
    for (name, browser) in [
        ("claude-acp", true),
        ("claude-acp", false),
        ("codex-acp", true),
    ] {
        let config = proxy_config(
            &agent(name),
            Arc::new(CountDenials::default()),
            (None, Arc::default()),
            browser,
        );
        assert_eq!(config.browser, browser, "{name}");
        assert_eq!(config.mode, ProxyMode::Open, "{name}");
    }
}

/// A session's proxy is a tier-C launch's until R17 PR-E passes `ListenerFor::TierB` for a
/// tier-B one: its agent listener, and its browser listener, stay outside `TIER_B_PORTS`, where
/// every pool user's static permit reaches (S15 PR7).
#[test]
fn the_session_proxy_listens_outside_the_tier_b_ports() {
    let mut config = config("claude-acp");
    assert_eq!(config.listener, ListenerFor::Other);
    config.block = loaded();
    let proxy = start_proxy(config).unwrap();
    assert!(!TIER_B_PORTS.contains(&proxy.port()), "{}", proxy.port());
    let browser = proxy.browser_port().unwrap();
    assert!(!TIER_B_PORTS.contains(&browser), "{browser}");
}

/// Claude's session reaches its API and the seed sites; Chrome's background hosts are silent;
/// the upstream is the system's; nothing is recorded.
#[test]
fn a_claude_session_allows_its_api_and_the_seed_sites() {
    let config = config("claude-acp");
    assert!(config.allow.contains("api.anthropic.com"));
    assert!(config.allow.contains("sci.gov.in"));
    assert!(config.allow.contains("delhihighcourt.nic.in"));
    assert!(!config.allow.contains("api.openai.com"));
    assert!(!config.allow.contains("example.com"));
    assert!(config.silent.contains("update.googleapis.com"));
    assert!(!config.silent.contains("sci.gov.in"));
    assert!(matches!(config.upstream, Upstream::System));
    assert!(config.record.is_none());
}

/// Codex's session gets `OpenAI`'s hosts; an agent without a kind gets only the seed sites.
#[test]
fn each_kind_gets_its_own_hosts() {
    let codex = config("codex-acp");
    assert!(codex.allow.contains("api.openai.com"));
    assert!(!codex.allow.contains("api.anthropic.com"));
    let other = config("fake");
    assert!(!other.allow.contains("api.openai.com"));
    assert!(!other.allow.contains("api.anthropic.com"));
    assert!(other.allow.contains("sci.gov.in"));
}

/// The sink counts refusals, a rate-cap summary as its count.
#[test]
fn the_denial_sink_counts_each_refusal() {
    let sink = CountDenials::default();
    sink.denied(Denial::Host {
        host: "client-site.example".to_owned(),
        port: 443,
        why: HostRefusal::NotAllowed,
        shared_hosting: false,
    });
    sink.denied(Denial::Dropped { count: 4 });
    assert_eq!(sink.count(), 5);
}

/// The config refuses from the block list it is given, the very `Current` the updater swaps;
/// with none, an empty one.
#[test]
fn the_config_shares_the_block_list_it_is_given() {
    let current = Arc::new(clauseo_blocklist::Current::default());
    let denials = || Arc::new(CountDenials::default());
    let shared = proxy_config(
        &agent("claude-acp"),
        denials(),
        (Some(Arc::clone(&current)), Arc::default()),
        true,
    );
    assert!(Arc::ptr_eq(&shared.block, &current));
    let none = proxy_config(
        &agent("claude-acp"),
        denials(),
        (None, Arc::default()),
        true,
    );
    assert!(!Arc::ptr_eq(&none.block, &current));
    assert!(none.block.get().is_unloaded());
}

/// A reporting sink posts each block-list refusal, the session's key and the host only, and
/// nothing for any other refusal; it counts them all.
#[test]
fn the_sink_posts_each_blocked_site_and_nothing_else() {
    let key = SessionKey {
        matter: clauseo_protocol::MatterId::new(uuid::Uuid::now_v7()).unwrap(),
        thread: clauseo_protocol::ThreadId::new(uuid::Uuid::now_v7()).unwrap(),
        launch: 3,
    };
    let (post, inputs) = async_channel::unbounded();
    let sink = CountDenials::reporting(key, post);
    let generated = clauseo_blocklist::Generated::parse("2026-11-07T06:00:00Z").unwrap();
    let blocked = || Denial::Blocked {
        host: "evil.example".to_owned(),
        port: 443,
        listed: "evil.example".to_owned(),
        tag: clauseo_blocklist::Tag::Phishing,
        generated,
    };
    sink.denied(blocked());
    sink.denied(Denial::Host {
        host: "client-site.example".to_owned(),
        port: 443,
        why: HostRefusal::NotAllowed,
        shared_hosting: false,
    });
    sink.denied(blocked());
    assert_eq!(sink.count(), 3);
    let mut posted = Vec::new();
    while let Ok(input) = inputs.try_recv() {
        let Input::SiteBlocked { session, site } = input else {
            panic!("{input:?}");
        };
        assert_eq!(session, key);
        assert_eq!(site.host, "evil.example");
        assert_eq!(site.tag, clauseo_blocklist::Tag::Phishing);
        assert_eq!(site.generated, generated);
        posted.push(site);
    }
    assert_eq!(posted.len(), 2, "once per refusal");
}

/// `run`'s key, with a fresh proxy guard on `TestProxy`, whose password is
/// `TestProxy::PASSWORD`.
fn with_proxy(run: &mut RunFolders) {
    let proxy: Arc<dyn crate::config::EgressProxy> = Arc::new(crate::TestProxy(8123));
    let credential = proxy.credential_for(run.key);
    run.egress = Some(Arc::new(EgressGuard {
        proxy,
        key: run.key,
        denials: Arc::new(CountDenials::default()),
        credential,
    }));
}

fn key() -> SessionKey {
    SessionKey {
        matter: clauseo_protocol::MatterId::new(uuid::Uuid::now_v7()).unwrap(),
        thread: clauseo_protocol::ThreadId::new(uuid::Uuid::now_v7()).unwrap(),
        launch: 1,
    }
}

/// The password goes to `<run>/proxy-secret`, its 64 hex characters and a line end, private to
/// the user; the proxy stays open.
#[test]
fn the_password_goes_to_a_private_file_in_the_run_folder() {
    let scratch = clauseo_connectors::fake::ScratchDir::new("proxy-secret").unwrap();
    let data = scratch.path().join("data");
    crate::write::create_dir_all(&data).unwrap();
    let mut run = RunFolders::new(&data, key());
    run.make(&data, &RunSweep::default()).unwrap();
    with_proxy(&mut run);
    write_secret(&mut run);
    assert!(run.egress.is_some());
    assert_eq!(run.proxy_secret, run.root.join("proxy-secret"));
    let text = std::fs::read_to_string(&run.proxy_secret).unwrap();
    assert_eq!(text, format!("{}\n", crate::TestProxy::PASSWORD));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&run.proxy_secret)
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "{mode:o}");
    }
}

/// A file that cannot be written closes the proxy again: the browser is then left out with
/// `NoProxy` (`agent::loads::browser`). No proxy, no file.
#[test]
fn a_password_file_not_written_closes_the_proxy() {
    let scratch = clauseo_connectors::fake::ScratchDir::new("proxy-secret-fails").unwrap();
    let data = scratch.path().join("data");
    crate::write::create_dir_all(&data).unwrap();
    let mut run = RunFolders::new(&data, key());
    run.make(&data, &RunSweep::default()).unwrap();
    // Its folder is a file: nothing can be written under it.
    let blocker = run.root.join("not-a-folder");
    crate::write::write_file(&blocker, b"x").unwrap();
    run.proxy_secret = blocker.join("proxy-secret");
    with_proxy(&mut run);
    write_secret(&mut run);
    assert!(run.egress.is_none());
    assert!(!run.proxy_secret.exists());

    let mut none = RunFolders::new(&data, key());
    write_secret(&mut none);
    assert!(none.egress.is_none());
    assert!(!none.proxy_secret.exists());
}

/// The session's end removes the password file (`end`, in the run folder's removal) whether or
/// not a guard read it; a missing file is fine, and a folder at the name stays.
#[test]
fn the_session_end_removes_the_password_file() {
    let scratch = clauseo_connectors::fake::ScratchDir::new("proxy-secret-end").unwrap();
    let data = scratch.path().join("data");
    crate::write::create_dir_all(&data).unwrap();
    let mut run = RunFolders::new(&data, key());
    run.make(&data, &RunSweep::default()).unwrap();
    with_proxy(&mut run);
    write_secret(&mut run);
    assert!(run.proxy_secret.is_file());
    end(&run);
    assert!(run.proxy_secret.symlink_metadata().is_err(), "not removed");
    end(&run);
    crate::write::create_dir_all(&run.proxy_secret).unwrap();
    end(&run);
    assert!(run.proxy_secret.is_dir(), "a folder at the name is left");
}

/// A link planted at the password file's name goes itself at the session's end; the file it
/// points at is never followed, so it keeps its bytes.
#[cfg(unix)]
#[test]
fn the_session_end_removes_a_link_at_the_name_never_its_target() {
    let scratch = clauseo_connectors::fake::ScratchDir::new("proxy-secret-link").unwrap();
    let data = scratch.path().join("data");
    crate::write::create_dir_all(&data).unwrap();
    let run = RunFolders::new(&data, key());
    run.make(&data, &RunSweep::default()).unwrap();
    let target = scratch.path().join("target");
    crate::write::write_file(&target, b"keep me").unwrap();
    crate::write::symlink(&target, &run.proxy_secret).unwrap();
    end(&run);
    assert!(
        run.proxy_secret.symlink_metadata().is_err(),
        "the link stayed"
    );
    assert_eq!(std::fs::read(&target).unwrap(), b"keep me");
}
