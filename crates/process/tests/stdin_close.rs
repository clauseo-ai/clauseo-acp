//! Closing [`clauseo_process::AgentStdin`] by its own `close`, against `fake_agent`'s `idle`
//! scenario (exits 0 when its stdin ends).

use std::io;

use async_io::block_on;
use clauseo_process::{LaunchSpec, Launched, SpawnOptions, spawn_in_tree};
use futures::{AsyncWriteExt, join};

/// Once the writer closes the agent's stdin, every later write and flush fails with
/// `BrokenPipe`, closing it again is fine, and the agent reads the end of its input.
#[test]
fn closed_stdin_refuses_writes_and_flushes_and_ends_the_agents_input() {
    let bin = clauseo_fake_agent::bin_path().expect(
        "fake_agent binary; `-p clauseo-process` does not build it: run `cargo build -p clauseo-fake_agent` first",
    );
    let spec = LaunchSpec::new(
        bin,
        vec!["--scenario".into(), "idle".into()],
        std::collections::BTreeMap::new(),
        env!("CARGO_MANIFEST_DIR").into(),
    )
    .unwrap();
    let Launched {
        mut stdin,
        stdout: _stdout,
        stderr,
        mut handle,
        ..
    } = spawn_in_tree(&spec, &SpawnOptions::new()).unwrap();
    let (report, pumped) = block_on(async {
        stdin.close().await.unwrap();
        let write = stdin.write_all(b"{}\n").await;
        assert_eq!(write.unwrap_err().kind(), io::ErrorKind::BrokenPipe);
        let flush = stdin.flush().await;
        assert_eq!(flush.unwrap_err().kind(), io::ErrorKind::BrokenPipe);
        stdin.close().await.unwrap();
        join!(handle.exited(), stderr.drain())
    });
    pumped.unwrap();
    // `idle` exits 0 only when its stdin ends.
    assert_eq!(report.code(), Some(0));
}
