//! `fake_agent` in the jail: an ACP handshake and the writes that must succeed, in the matter
//! and in its notes folder.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::time::Duration;

use clauseo_process::{AgentStdin, LaunchSpec, Launched, Launcher};
use futures::StreamExt;
use futures::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use serde_json::{Value, json};

use crate::support::{World, launcher};

#[test]
fn fake_agent_writes_in_the_matter_and_its_notes() {
    let w = World::new();
    let bin = clauseo_fake_agent::bin_path()
        .expect("fake_agent binary: run `cargo build -p clauseo-fake_agent` first");
    let policy = w.builder().runtime(&bin).build().unwrap();
    let matter = policy.matter().to_owned();
    let notes = policy.notes().unwrap().to_owned();
    let (launcher, denials) = launcher(policy);
    let args = vec![
        OsString::from("--scenario"),
        OsString::from("sandbox-probe"),
    ];
    let spec = LaunchSpec::new(bin, args, BTreeMap::new(), matter.clone()).unwrap();
    let Launched {
        mut stdin,
        stdout,
        stderr,
        mut handle,
        ..
    } = launcher.launch(&spec).unwrap();
    let mut lines = BufReader::new(stdout).lines();
    let script = async {
        let mut call = Call {
            stdin: &mut stdin,
            lines: &mut lines,
        };
        let answered = async {
            call.answer(
                1,
                "initialize",
                json!({"protocolVersion": 1, "clientCapabilities": {}}),
            )
            .await?;
            let new = json!({"cwd": matter, "mcpServers": []});
            assert_eq!(
                call.answer(2, "session/new", new).await?["sessionId"],
                "sess-1"
            );
            let prompt = json!({"sessionId": "sess-1", "prompt": [{"type": "text", "text": "go"}]});
            assert_eq!(
                call.answer(3, "session/prompt", prompt).await?["stopReason"],
                "end_turn"
            );
            let new = json!({"cwd": notes, "mcpServers": []});
            assert_eq!(
                call.answer(4, "session/new", new).await?["sessionId"],
                "sess-2"
            );
            let prompt = json!({"sessionId": "sess-2", "prompt": [{"type": "text", "text": "go"}]});
            assert_eq!(
                call.answer(5, "session/prompt", prompt).await?["stopReason"],
                "end_turn"
            );
            Ok::<(), u64>(())
        }
        .await;
        stdin.close().await.unwrap();
        (answered, handle.exited().await)
    };
    let ((answered, report), pumped) =
        async_io::block_on(async { futures::join!(script, stderr.drain()) });
    pumped.unwrap();
    if let Err(id) = answered {
        // The watcher reports a denial a moment after it happens, so give it a little time.
        let mut seen = Vec::new();
        while let Ok(d) = denials.recv_timeout(Duration::from_secs(2)) {
            seen.push(d);
        }
        panic!(
            "the pipes closed before the answer to request {id}: exit code {:?}, signal {:?}, \
             denials {seen:#?}, stderr:\n{}",
            report.code(),
            report.signal(),
            report.stderr_tail()
        );
    }
    assert_eq!(
        report.code(),
        Some(clauseo_fake_agent::EXIT_OK),
        "{}",
        report.stderr_tail()
    );
    assert_eq!(
        std::fs::read_to_string(matter.join("probe/in-matter.txt")).unwrap(),
        "written in the jail\n"
    );
    assert_eq!(
        std::fs::read_to_string(notes.join("from-the-jail.txt")).unwrap(),
        "a note\n"
    );
}

/// The client side of the ACP pipes.
struct Call<'a, L> {
    stdin: &'a mut AgentStdin,
    lines: &'a mut L,
}

impl<L: futures::Stream<Item = std::io::Result<String>> + Unpin> Call<'_, L> {
    /// Sends request `id` and returns its result, or `Err(id)` when the agent's pipes close first.
    async fn answer(&mut self, id: u64, method: &str, params: Value) -> Result<Value, u64> {
        let request = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        // A write fails with a broken pipe once the agent has exited: report it like an
        // early end of stdout, with the exit and stderr.
        let sent = self
            .stdin
            .write_all(format!("{request}\n").as_bytes())
            .await;
        if sent.and(self.stdin.flush().await).is_err() {
            return Err(id);
        }
        let Some(line) = self.lines.next().await else {
            return Err(id);
        };
        let line = line.unwrap();
        let answer: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(answer["id"], id, "{answer}");
        Ok(answer["result"].clone())
    }
}
