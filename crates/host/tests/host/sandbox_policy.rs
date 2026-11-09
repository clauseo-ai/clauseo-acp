//! A jailed launch's policy and environment through the launcher seam (R17 PR-D): the
//! reservation comes first and its ports go into the policy; a launch failing after it releases
//! it; every other matter of the registry is hidden, open, never opened or forgotten; the
//! session's proxy opens with the tier-B agent listener, no token-free browser listener and the
//! host's block list; the agent's proxy and CA
//! variables are explicit in a jailed spec (Windows' runner passes nothing inherited) and absent
//! at tier C; a jailed Codex is refused before anything is reserved while it would start in its
//! ask mode (R17 PR-D2c starts it in the jailed preset).

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::Path;
use std::sync::Arc;

use clauseo_blocklist::Current;
use clauseo_host::{BlockListSeams, NoProfile, ProfileHook, TestProxy};
use clauseo_profile::PacksWritten;
use clauseo_protocol::{AgentId, CommandOutcome, ErrorCode, HostCommand, StopReason, StreamId};
use clauseo_sandbox::ports::ListenerFor;

use crate::codex::{CODEX, codex_dir};
use crate::egress_block::{FIRST, LISTED, list, no_fetch};
use crate::profile_rig::CLAUDE;
use crate::sandbox_rig::{Answer, Env, Fake, RESERVED_PORTS, canonical, dir, failed, start_with};
use crate::support::{Dir, FAKE, Scenario, matter_with_thread};

/// The test proxy's port: its browser and agent listener's.
const PROXY_PORT: u16 = 47_001;

/// `name` in `env`, as text.
fn var<'a>(env: &'a Env, name: &str) -> Option<&'a str> {
    env.get(&OsString::from(name))
        .and_then(|value| value.to_str())
}

/// The CA variables a jailed launch sets, each at the session's bundle in `env`.
fn assert_ca_vars(env: &Env, data: &Path) {
    let bundle = var(env, "SSL_CERT_FILE").expect("SSL_CERT_FILE");
    assert!(bundle.ends_with("ca-bundle.pem"), "{bundle}");
    assert!(
        Path::new(bundle).starts_with(canonical(&data.join("run")))
            || Path::new(bundle).starts_with(data.join("run")),
        "{bundle}"
    );
    for name in ["CODEX_CA_CERTIFICATE", "NODE_EXTRA_CA_CERTS"] {
        assert_eq!(var(env, name), Some(bundle), "{name}");
    }
}

#[test]
fn a_jailed_launch_reserves_first_and_its_policy_takes_the_reserved_and_proxy_ports() {
    let dir = dir("sandbox_policy_ports", "prompt-stream");
    let (fake, _) = Fake::new(&dir, Answer::Launch(Vec::new()));
    let proxy = Arc::new(TestProxy::new(PROXY_PORT));
    let wired = Arc::clone(&proxy);
    let current = Arc::new(Current::new(list(FIRST, &[LISTED])));
    let seams = BlockListSeams::new(Arc::clone(&current), None, no_fetch());
    let (host, _, _) = start_with(&dir, Some(Arc::clone(&fake)), move |config| {
        seams.apply(
            config
                .with_egress_proxy(wired)
                .with_inherited_env(Vec::new()),
        )
    });
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[("a.txt", "x\n")], FAKE);
    // Another matter of the registry, never opened: hidden all the same.
    let other = dir.folder("other", &[("b.txt", "y\n")]);
    let _other = host.create_matter("other", &other);
    // And one forgotten: still hidden.
    let gone = dir.folder("gone", &[("c.txt", "z\n")]);
    let forgotten = host.create_matter("gone", &gone);
    assert_eq!(
        host.ok(HostCommand::ForgetMatter { matter: forgotten }),
        CommandOutcome::Done
    );
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Look at the matter.");
    assert_eq!(
        events.until_turn_ended(turn).stop,
        StopReason::EndTurn,
        "{}",
        events.dump()
    );

    let calls = fake.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].matter, matter);
    assert_eq!(
        (fake.reserved(), fake.released()),
        (1, 1),
        "reserved once, released once"
    );
    let policy = &calls[0].policy;
    for port in RESERVED_PORTS.into_iter().chain([PROXY_PORT]) {
        assert!(
            policy.proxy_ports().contains(&port),
            "{:?}",
            policy.proxy_ports()
        );
    }
    assert!(!policy.keychain());
    for hidden in [&other, &gone, dir.data_dir()] {
        assert!(
            policy.hidden().contains(&canonical(hidden)),
            "{:?}",
            policy.hidden()
        );
    }

    // The session's proxy: opened once, for this session, with the tier-B agent listener, no
    // token-free browser listener (no browser here) and the host's block list.
    let opened = proxy.opened();
    assert_eq!(opened.len(), 1, "{opened:?}");
    assert_eq!(opened[0].session, calls[0].session);
    assert_eq!(opened[0].listener, ListenerFor::TierB);
    assert!(
        !opened[0].browser,
        "no browser listener without the browser"
    );
    assert!(Arc::ptr_eq(&opened[0].block, &current), "the seams' list");

    // The proxy's and the CA bundle's variables, explicit with nothing inherited.
    let envs = fake.envs();
    assert_eq!(envs.len(), 1);
    let env = &envs[0];
    assert_eq!(
        var(env, "HTTPS_PROXY"),
        Some(format!("http://clauseo:test@127.0.0.1:{PROXY_PORT}").as_str())
    );
    assert_eq!(var(env, "ALL_PROXY"), None, "removed");
    assert_ca_vars(env, dir.data_dir());
}

#[test]
fn a_launch_failing_after_its_reservation_releases_it() {
    let dir = dir("sandbox_policy_release", "prompt-stream");
    let (fake, seam_launched) = Fake::new(&dir, Answer::Fail);
    let (host, host_launched, _) = start_with(&dir, Some(Arc::clone(&fake)), |config| config);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("a.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Look at the matter.");
    events.until_turn_ended(turn);
    let (code, _) = failed(&events, turn).expect("the launch failed");
    assert_eq!(code, ErrorCode::AgentLaunchFailed);
    assert_eq!((fake.reserved(), fake.released()), (1, 1));
    let launched = seam_launched.load(std::sync::atomic::Ordering::SeqCst)
        + host_launched.load(std::sync::atomic::Ordering::SeqCst);
    assert_eq!(launched, 0);
}

#[test]
fn tier_c_sets_no_proxy_or_ca_variable() {
    let dir = dir("sandbox_policy_tier_c", "prompt-stream");
    let proxy = Arc::new(TestProxy::new(PROXY_PORT));
    let wired = Arc::clone(&proxy);
    let (host, _, host_launcher) = start_with(&dir, None, move |config| {
        config
            .with_egress_proxy(wired)
            .with_inherited_env(Vec::new())
    });
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("a.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Look at the matter.");
    assert_eq!(events.until_turn_ended(turn).stop, StopReason::EndTurn);
    let envs = host_launcher.envs();
    assert_eq!(envs.len(), 1);
    for name in [
        "HTTPS_PROXY",
        "SSL_CERT_FILE",
        "CODEX_CA_CERTIFICATE",
        "NODE_EXTRA_CA_CERTS",
    ] {
        assert_eq!(var(&envs[0], name), None, "{name}");
    }
    // Tier C without the browser opens no proxy at all.
    assert!(proxy.opened().is_empty());
}

/// A profile hook giving Claude a stored key, as desktop's does.
struct KeyHook;

impl ProfileHook for KeyHook {
    fn session_meta(
        &self,
        agent: &AgentId,
        packs: Option<&PacksWritten>,
    ) -> Option<serde_json::Map<String, serde_json::Value>> {
        NoProfile.session_meta(agent, packs)
    }

    fn launch_env(&self, _agent: &AgentId) -> BTreeMap<OsString, OsString> {
        BTreeMap::from([("ANTHROPIC_API_KEY".into(), "sk-test-key".into())])
    }
}

/// Windows' tier-B runner (#792) passes the agent only the spec's own variables: a jailed
/// Claude's spec built with nothing inherited carries its state folder, its store, its key, the
/// proxy's and the CA bundle's variables.
#[test]
fn a_jailed_claude_s_spec_carries_every_variable_it_needs_with_nothing_inherited() {
    let dir = Dir::new(
        "sandbox_policy_claude_env",
        &[(CLAUDE, Scenario::Bundled("claude-prompt-stream"))],
    );
    let (fake, _) = Fake::new(&dir, Answer::Launch(Vec::new()));
    let (host, _, _) = start_with(&dir, Some(Arc::clone(&fake)), |config| {
        config
            .with_egress_proxy(Arc::new(TestProxy::new(PROXY_PORT)))
            .with_inherited_env(Vec::new())
            .with_profile_hook(Box::new(KeyHook))
    });
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("a.txt", "x\n")], CLAUDE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Look at the matter.");
    assert_eq!(
        events.until_turn_ended(turn).stop,
        StopReason::EndTurn,
        "{}",
        events.dump()
    );
    let envs = fake.envs();
    let env = envs.first().expect("one jailed launch");
    for name in [
        "CLAUDE_CONFIG_DIR",
        "CLAUDE_SECURESTORAGE_CONFIG_DIR",
        "HTTPS_PROXY",
    ] {
        assert!(
            var(env, name).is_some_and(|value| !value.is_empty()),
            "{name}"
        );
    }
    assert_eq!(var(env, "ANTHROPIC_API_KEY"), Some("sk-test-key"));
    assert_ca_vars(env, dir.data_dir());
    // Its policy grants Clauseo's store, no Keychain.
    let policy = &fake.calls()[0].policy;
    assert!(policy.claude_store().is_some());
    assert!(!policy.keychain());
}

/// Until R17 PR-D2c starts a jailed Codex in the jailed preset, a jailed Codex launch is refused
/// before anything is reserved or any launcher made: it would start in its ask mode.
#[test]
fn a_jailed_codex_is_refused_before_any_reservation_or_launcher() {
    let dir = codex_dir("sandbox_policy_codex", "codex-handshake");
    let (fake, seam_launched) = Fake::new(&dir, Answer::Launch(Vec::new()));
    let (host, host_launched, _) = start_with(&dir, Some(Arc::clone(&fake)), |config| config);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("a.txt", "x\n")], CODEX);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Look at the matter.");
    events.until_turn_ended(turn);
    let (code, detail) = failed(&events, turn).expect("refused");
    assert_eq!(code, ErrorCode::AgentLaunchFailed, "{detail:?}");
    assert_eq!(fake.reserved(), 0, "nothing reserved");
    assert!(fake.calls().is_empty(), "no launcher made");
    let launched = seam_launched.load(std::sync::atomic::Ordering::SeqCst)
        + host_launched.load(std::sync::atomic::Ordering::SeqCst);
    assert_eq!(launched, 0);
}
