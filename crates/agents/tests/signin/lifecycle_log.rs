//! The sign-in jobs' lines on the lifecycle target, which host.log keeps: each job's start
//! with its agent, kind and why, and its end with its result, code and `took_ms`; never the
//! account's email or a path. The jobs run on threads of their own, so the lines are caught by
//! a subscriber for the whole test binary: other tests' jobs log there too, which only adds
//! lines, and none of them may hold an email or a path either.

use std::sync::{Arc, Mutex, OnceLock};

use clauseo_agents::{Method, SigninCode, SigninState};

use crate::support::{Fixture, claude, lock};

/// What the binary's subscriber wrote.
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

/// The binary's subscriber, set once.
fn captured() -> Lines {
    static LINES: OnceLock<Lines> = OnceLock::new();
    LINES
        .get_or_init(|| {
            let lines = Lines::default();
            let writer = lines.clone();
            let subscriber = tracing_subscriber::fmt()
                .with_writer(move || writer.clone())
                .with_ansi(false)
                .finish();
            tracing::subscriber::set_global_default(subscriber).expect("one subscriber");
            lines
        })
        .clone()
}

/// The lifecycle target's job lines so far.
fn job_lines(lines: &Lines) -> Vec<String> {
    String::from_utf8_lossy(&lock(&lines.0))
        .lines()
        .filter(|line| line.contains("clauseo_agents::lifecycle") && line.contains("sign-in job"))
        .map(str::to_owned)
        .collect()
}

/// Whether one of `lines` holds every one of `fields`.
fn has(lines: &[String], fields: &[&str]) -> bool {
    lines
        .iter()
        .any(|line| fields.iter().all(|field| line.contains(field)))
}

#[test]
fn each_job_logs_its_start_and_its_end_without_the_email_or_a_path() {
    let lines = captured();
    let fx = Fixture::new("signin-lifecycle-log", "ok-after:0");
    let (agents, events) = fx.start();
    agents.sign_in(&claude(), Method::ClaudeAi).unwrap();
    assert!(matches!(events.until_end().1, SigninState::SignedIn { .. }));
    agents.sign_out(&claude()).unwrap();
    assert!(matches!(
        events.until_end().1,
        SigninState::SignedOut { .. }
    ));
    agents.check_status(&claude()).unwrap();
    events.until_end();
    let logged = job_lines(&lines);
    let agent = "agent=\"claude-acp\"";
    let want: [&[&str]; 6] = [
        &[
            "job started",
            agent,
            "kind=\"sign_in\"",
            "why=\"claude_ai\"",
        ],
        &[
            "job ended",
            agent,
            "kind=\"sign_in\"",
            "result=\"signed_in\"",
            "took_ms=",
        ],
        &["job started", agent, "kind=\"sign_out\""],
        &[
            "job ended",
            agent,
            "kind=\"sign_out\"",
            "result=\"signed_out\"",
            "code=\"none\"",
        ],
        &["job started", agent, "kind=\"status\"", "why=\"check\""],
        &["job ended", agent, "kind=\"status\"", "took_ms="],
    ];
    for fields in want {
        assert!(
            has(&logged, fields),
            "no line with {fields:?}:\n{logged:#?}"
        );
    }
    let text = String::from_utf8_lossy(&lock(&lines.0)).into_owned();
    assert!(!text.contains("test@example.com"), "{text}");
    let data = fx.data.path().to_string_lossy().into_owned();
    assert!(!text.contains(&data), "{text}");
}

#[test]
fn a_cancelled_sign_in_logs_cancelled_with_its_code() {
    let lines = captured();
    let fx = Fixture::new("signin-lifecycle-cancel", "never");
    let (agents, events) = fx.start();
    agents.sign_in(&claude(), Method::ClaudeAi).unwrap();
    events.until(|state| matches!(state, SigninState::Waiting { .. }));
    crate::support::eventually("the login started", || !fx.runs_of("login").is_empty());
    agents.cancel_sign_in(&claude()).unwrap();
    let (_, last) = events.until_end();
    assert!(matches!(
        last,
        SigninState::Failed {
            code: SigninCode::Cancelled,
            ..
        }
    ));
    let fields = [
        "job ended",
        "kind=\"sign_in\"",
        "result=\"cancelled\"",
        "code=\"signin.cancelled\"",
    ];
    let logged = job_lines(&lines);
    assert!(has(&logged, &fields), "{logged:#?}");
    let data = fx.data.path().to_string_lossy().into_owned();
    assert!(!String::from_utf8_lossy(&lock(&lines.0)).contains(&data));
}
