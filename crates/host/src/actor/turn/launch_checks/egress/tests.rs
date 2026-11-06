//! The session proxy's config: what it allows and refuses silently, and the denial sink, which
//! counts and never logs the host.

use std::io::Write;
use std::sync::{Arc, Mutex};

use clauseo_protocol::{AgentId, ThreadId};
use clauseo_sandbox::{Denial, DenialSink as _, HostRefusal, Upstream};

use super::{CountDenials, proxy_config};
use crate::pipeline::tests::support::id;

fn agent(name: &str) -> AgentId {
    AgentId::new(name).unwrap()
}

/// Claude's session reaches its API and the seed sites; Chrome's background hosts are silent;
/// the upstream is the system's; nothing is recorded.
#[test]
fn a_claude_session_allows_its_api_and_the_seed_sites() {
    let config = proxy_config(&agent("claude-acp"), id::<ThreadId>(1));
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
    let codex = proxy_config(&agent("codex-acp"), id::<ThreadId>(1));
    assert!(codex.allow.contains("api.openai.com"));
    assert!(!codex.allow.contains("api.anthropic.com"));
    let other = proxy_config(&agent("fake"), id::<ThreadId>(1));
    assert!(!other.allow.contains("api.openai.com"));
    assert!(!other.allow.contains("api.anthropic.com"));
    assert!(other.allow.contains("sci.gov.in"));
}

/// A log capture.
#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl Write for Captured {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// The sink counts refusals (a rate-cap summary as its count) and logs the count, the agent and
/// the thread, never the host it was asked for.
#[test]
fn the_denial_sink_counts_and_never_logs_the_host() {
    let captured = Captured::default();
    let writer = captured.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .with_writer(move || writer.clone())
        .with_max_level(tracing::Level::TRACE)
        .finish();
    let sink = CountDenials {
        agent: agent("claude-acp"),
        thread: id::<ThreadId>(3),
        count: std::sync::atomic::AtomicU64::new(0),
    };
    tracing::subscriber::with_default(subscriber, || {
        sink.denied(Denial::Host {
            host: "secret-client-site.example".to_owned(),
            port: 443,
            why: HostRefusal::NotAllowed,
            shared_hosting: false,
        });
        sink.denied(Denial::Dropped { count: 4 });
    });
    assert_eq!(sink.count(), 5);
    let text = String::from_utf8(captured.0.lock().unwrap().clone()).unwrap();
    assert!(text.contains("count=5"), "{text}");
    assert!(text.contains("claude-acp"), "{text}");
    assert!(!text.contains("secret-client-site"), "{text}");
}
