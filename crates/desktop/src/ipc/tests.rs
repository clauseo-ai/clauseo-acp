use std::sync::{Arc, Mutex};

use futures::channel::oneshot;

use super::{Ledger, TARGET, UNLOGGED};
use crate::bridge::lock;

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

/// Runs `run` under a subscriber that writes every line, and gives the lines.
fn captured(run: impl FnOnce()) -> Vec<String> {
    let lines = Lines::default();
    let writer = lines.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(move || writer.clone())
        .with_ansi(false)
        .finish();
    tracing::subscriber::with_default(subscriber, run);
    let text = String::from_utf8(lock(&lines.0).clone()).unwrap();
    text.lines().map(str::to_owned).collect()
}

/// host.log keeps info lines only from a `clauseo_*` crate's `::lifecycle` target.
#[test]
fn the_target_is_one_host_log_keeps() {
    assert!(TARGET.starts_with("clauseo_"));
    assert!(TARGET.ends_with("::lifecycle"));
}

/// One line per call, each with its name, times and outcome and no argument; past the cap one
/// line says so, and nothing more.
#[test]
fn each_call_logs_one_line_up_to_the_cap() {
    let ledger = Ledger::new(3);
    let lines = captured(|| {
        futures::executor::block_on(async {
            let secret = "C:\\Users\\someone\\Matters";
            let answer = ledger
                .timed("query", async { Ok::<_, String>(secret.len()) })
                .await;
            assert_eq!(answer, Ok(secret.len()));
            let refused = ledger
                .timed("dispatch", async { Err::<(), _>(secret.to_owned()) })
                .await;
            assert!(refused.is_err());
            for _ in 0..4 {
                ledger
                    .timed("subscribe", async { Ok::<_, ()>(()) })
                    .await
                    .unwrap();
            }
        });
    });
    assert_eq!(lines.len(), 4, "{lines:#?}");
    for line in &lines[..3] {
        assert!(line.contains(TARGET), "{line}");
        assert!(line.contains("ipc call"), "{line}");
        for field in ["at_ms=", "took_ms=", "waited_ms=-1", "running=1"] {
            assert!(line.contains(field), "{field} in {line}");
        }
        assert!(
            !line.contains("Matters"),
            "no argument or error text: {line}"
        );
    }
    assert!(lines[0].contains("command=\"query\"") && lines[0].contains("outcome=\"ok\""));
    assert!(lines[1].contains("command=\"dispatch\"") && lines[1].contains("outcome=\"error\""));
    assert!(lines[2].contains("command=\"subscribe\""));
    assert!(
        lines[3].contains("past the cap") && lines[3].contains("cap=3"),
        "{}",
        lines[3]
    );
}

/// A call started while another is still running says two are running; a hand-over stamped
/// before the body gives how long the body waited for it.
#[test]
fn a_line_counts_the_calls_running_and_the_wait_for_its_body() {
    let ledger = Ledger::new(10);
    let lines = captured(|| {
        futures::executor::block_on(async {
            let (answer, answered) = oneshot::channel::<()>();
            let mut slow = Box::pin(ledger.timed("subscribe", answered));
            assert!(futures::poll!(slow.as_mut()).is_pending());
            // handed over 20 ms before its body starts
            let handed = crate::open::now()
                .checked_sub(std::time::Duration::from_millis(20))
                .unwrap();
            ledger.received_at("pick_folder_or_choose", handed);
            ledger
                .timed("pick_folder_or_choose", async { Ok::<_, ()>(()) })
                .await
                .unwrap();
            answer.send(()).unwrap();
            slow.await.unwrap();
        });
    });
    assert_eq!(lines.len(), 2, "{lines:#?}");
    let click = &lines[0];
    assert!(
        click.contains("command=\"pick_folder_or_choose\""),
        "{click}"
    );
    assert!(click.contains("running=2"), "{click}");
    let waited: i64 = click
        .split("waited_ms=")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .unwrap()
        .parse()
        .unwrap();
    assert!(waited >= 20, "{click}");
    assert!(lines[1].contains("command=\"subscribe\"") && lines[1].contains("running=1"));
}

/// A body dropped before it answered still logs its line, as `dropped`, and leaves the count.
#[test]
fn a_dropped_call_logs_dropped() {
    let ledger = Ledger::new(10);
    let lines = captured(|| {
        futures::executor::block_on(async {
            let mut never =
                Box::pin(ledger.timed("query", futures::future::pending::<Result<(), ()>>()));
            assert!(futures::poll!(never.as_mut()).is_pending());
            drop(never);
            ledger
                .timed("subscribe", async { Ok::<_, ()>(()) })
                .await
                .unwrap();
        });
    });
    assert!(lines[0].contains("outcome=\"dropped\""), "{}", lines[0]);
    assert!(lines[1].contains("running=1"), "{}", lines[1]);
}

/// `ack` calls, which the page sends for each batch of events, are never logged and never
/// count toward the cap, nor do their hand-overs take a stamp; the other calls log as before, up
/// to the cap and its one line.
#[test]
fn ack_calls_neither_log_nor_count_toward_the_cap() {
    assert_eq!(UNLOGGED, "ack");
    let ledger = Ledger::new(2);
    let lines = captured(|| {
        futures::executor::block_on(async {
            for _ in 0..50 {
                ledger.received("ack");
                ledger
                    .timed("ack", async { Ok::<_, ()>(()) })
                    .await
                    .unwrap();
            }
            for _ in 0..3 {
                ledger
                    .timed("query", async { Ok::<_, ()>(()) })
                    .await
                    .unwrap();
            }
            ledger
                .timed("ack", async { Ok::<_, ()>(()) })
                .await
                .unwrap();
        });
    });
    assert!(lock(&ledger.stamps).is_empty(), "no stamp for an ack");
    assert_eq!(lines.len(), 3, "{lines:#?}");
    assert!(
        lines.iter().all(|line| !line.contains("\"ack\"")),
        "{lines:#?}"
    );
    assert!(lines[0].contains("command=\"query\"") && lines[1].contains("command=\"query\""));
    assert!(
        lines[2].contains("past the cap") && lines[2].contains("cap=2"),
        "{}",
        lines[2]
    );
}
