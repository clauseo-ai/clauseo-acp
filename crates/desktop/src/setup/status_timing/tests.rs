use std::sync::{Arc, Mutex};

use clauseo_protocol::{AgentId, LoginFrom, SetupEvent, SignIn};

use super::TARGET;
use crate::bridge::lock;
use crate::setup::SetupEvents;

/// What a test's subscriber wrote.
#[derive(Clone, Default)]
struct Lines(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Lines {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        lock(&self.0).extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// host.log keeps info lines only from a `clauseo_*` crate's `::lifecycle` target.
#[test]
fn the_target_is_one_host_log_keeps() {
    assert!(TARGET.starts_with("clauseo_") && TARGET.ends_with("::lifecycle"));
}

/// The check asked for, then the agent's next sign-in state published: one line each, the
/// answer with how long it took and the state's kind, and the published event's own line; no
/// email. A second state for the same agent answers nothing more.
#[test]
fn a_status_check_logs_when_it_was_asked_and_when_its_state_came() {
    let lines = Lines::default();
    let writer = lines.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(move || writer.clone())
        .with_ansi(false)
        .finish();
    let claude = AgentId::new("claude-acp").unwrap();
    let found = || SetupEvent::SignInChanged {
        agent: claude.clone(),
        state: SignIn::LoginFound {
            from: LoginFrom::ClaudeCode,
            email: Some("someone@example.com".to_owned()),
        },
    };
    tracing::subscriber::with_default(subscriber, || {
        let events = SetupEvents::default();
        super::asked(&claude, "installed");
        events.publish(found());
        events.publish(found());
    });
    let text = String::from_utf8(lock(&lines.0).clone()).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert!(!text.contains("someone@example.com"), "{text}");
    let asked: Vec<_> = lines.iter().filter(|l| l.contains("check asked")).collect();
    let answered: Vec<_> = lines
        .iter()
        .filter(|l| l.contains("check answered"))
        .collect();
    assert_eq!(asked.len(), 1, "{text}");
    assert!(asked[0].contains("agent=\"claude-acp\"") && asked[0].contains("why=\"installed\""));
    assert_eq!(answered.len(), 1, "{text}");
    for field in ["at_ms=", "took_ms=", "state=\"login_found\"", TARGET] {
        assert!(answered[0].contains(field), "{field} in {}", answered[0]);
    }
    let published = lines.iter().filter(|l| l.contains("setup event")).count();
    assert_eq!(published, 2, "{text}");
}

/// Each sign-in state that reaches the windows with a kind other than the agent's last logs
/// the change, old kind to new; the same kind again logs none. No email, no URL.
#[test]
fn a_sign_in_state_change_logs_its_old_and_new_kind() {
    let lines = Lines::default();
    let writer = lines.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(move || writer.clone())
        .with_ansi(false)
        .finish();
    // An agent no other test publishes for: the last kinds are kept for the whole run.
    let agent = AgentId::new("state-change-log-test").unwrap();
    let found = || SetupEvent::SignInChanged {
        agent: agent.clone(),
        state: SignIn::LoginFound {
            from: LoginFrom::Codex,
            email: Some("test@example.com".to_owned()),
        },
    };
    tracing::subscriber::with_default(subscriber, || {
        let events = SetupEvents::default();
        events.publish(SetupEvent::SignInChanged {
            agent: agent.clone(),
            state: SignIn::Checking,
        });
        events.publish(found());
        events.publish(found());
    });
    let text = String::from_utf8(lock(&lines.0).clone()).unwrap();
    assert!(!text.contains("test@example.com"), "{text}");
    let changes: Vec<&str> = text
        .lines()
        .filter(|line| line.contains("sign-in state changed"))
        .collect();
    assert_eq!(changes.len(), 2, "{text}");
    for (line, (from, to)) in changes
        .iter()
        .zip([("", "checking"), ("checking", "login_found")])
    {
        for field in [
            TARGET,
            "agent=\"state-change-log-test\"",
            &format!("from=\"{from}\""),
            &format!("to=\"{to}\""),
        ] {
            assert!(line.contains(field), "{field} in {line}");
        }
    }
}
