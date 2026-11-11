//! `Handle::exited_draining` off Windows: no stop spares a process there, so the wait still
//! reads the pipes to their end, as `exited` beside the reads did. A process that left the
//! run's group outlives the group's kill and writes after the run exited; the wait has its
//! line. The Windows case, a spared browser holding the pipes, is `spare_browser_windows.rs`.

#![cfg(target_os = "linux")]

use async_io::block_on;
use clauseo_process::{AsyncIoTimer, LaunchSpec, Launched, SpawnOptions, spawn_in_tree};
use futures::io::AsyncReadExt as _;

#[test]
fn the_wait_reads_the_pipes_to_their_end_after_the_exit() {
    // `setsid` puts the late writer in a session of its own, outside the group the exit kills;
    // the run waits for its mark, made once it is there, so the kill cannot come before.
    let mark = std::env::temp_dir().join(format!("clauseo-exit-drain-{}", std::process::id()));
    let mark = mark.display();
    let line = format!(
        "setsid sh -c 'touch {mark}; sleep 0.3; rm -f {mark}; echo late' & \
         while [ ! -e {mark} ]; do :; done; echo early"
    );
    let spec = LaunchSpec::new(
        "/bin/sh".into(),
        vec!["-c".into(), line.into()],
        std::collections::BTreeMap::new(),
        env!("CARGO_MANIFEST_DIR").into(),
    )
    .unwrap();
    let Launched {
        stdin,
        mut stdout,
        stderr,
        mut handle,
        ..
    } = spawn_in_tree(&spec, &SpawnOptions::new()).unwrap();
    drop(stdin);
    let mut out = Vec::new();
    let (report, ended) = block_on(async {
        let read = async {
            let mut chunk = [0_u8; 4096];
            while let Ok(n @ 1..) = stdout.read(&mut chunk).await {
                out.extend_from_slice(chunk.get(..n).unwrap_or_default());
            }
        };
        let errs = async {
            drop(stderr.drain().await);
        };
        let pipes = async {
            futures::future::join(read, errs).await;
        };
        Box::pin(handle.exited_draining(&AsyncIoTimer, pipes)).await
    });
    assert_eq!(report.code(), Some(0));
    assert!(ended, "the pipes reached their end");
    let text = String::from_utf8_lossy(&out);
    assert!(text.contains("early") && text.contains("late"), "{text}");
}
