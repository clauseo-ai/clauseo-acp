//! The runs of `session_sweep.rs`: the writer script, the shapes it is started in, the `/bin/sh`
//! agent that starts one and exits, and the sweep. Its own module, with `#[cfg(test)]`, so
//! clippy treats its helpers as test code.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use async_io::{Timer, block_on};
use clauseo_process::{
    AsyncIoTimer, LaunchSpec, Launched, SessionMark, SessionSwept, SpawnOptions, spawn_in_tree,
};

/// The writer, run as `sh -c "$WRITER_VAR" sh <folder>`: it marks `.up` once it runs (after
/// `setsid` where there is one), sleeps 3 s, then writes `late.txt`. It reaches the agent in its
/// environment, so no shape nests one quoted command inside another and no file is written.
const WRITER: &str = "touch \"$1/.up\"; sleep 3; echo x > \"$1/late.txt\"";
/// The variable that carries [`WRITER`].
const WRITER_VAR: &str = "SWEEP_TEST_WRITER";
/// How long the test waits for a writer that still runs to write.
pub(crate) const WAIT: Duration = Duration::from_secs(5);

/// What starts a command in a session of its own: `setsid(1)` on Linux; macOS has no such
/// command, so Perl's `POSIX::setsid` and `exec`. Double quotes only, so a shape can put it
/// inside a single-quoted `sh -c`.
#[cfg(target_os = "linux")]
const SETSID: &str = "setsid";
#[cfg(target_os = "macos")]
const SETSID: &str = "perl -MPOSIX -e \"POSIX::setsid() or die; exec @ARGV\" --";

/// No terminal and no pipe of the agent's, so the agent's pipes close when it exits.
const QUIET: &str = "</dev/null >/dev/null 2>&1";

/// The shapes, each a name and the line the agent runs before it exits, with the matter folder
/// as `$1`.
pub(crate) fn shapes() -> [(&'static str, String); 5] {
    let writer = format!("sh -c \"${WRITER_VAR}\" sh \"$1\" {QUIET}");
    [
        ("setsid", format!("{SETSID} {writer} &")),
        ("double fork", format!("({writer} &) &")),
        ("nohup", format!("nohup {writer} &")),
        (
            "fork, setsid, fork",
            format!("sh -c '( {SETSID} {writer} & ) &' sh \"$1\" {QUIET};"),
        ),
        (
            "own-session shell, nohup",
            format!("{SETSID} sh -c 'nohup {writer} & exit 0' sh \"$1\" {QUIET} &"),
        ),
    ]
}

/// A matter folder named like a lawyer's under this test's own temp folder.
pub(crate) fn matter(test: &str, name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("clauseo-sweep-{test}-{}", std::process::id()));
    drop(crate::write::remove_dir_all(&root));
    let folder = root.join(name);
    crate::write::create_dir_all(&folder).unwrap();
    folder
}

/// Removes the test's own scratch folder.
pub(crate) fn remove(folder: &Path) {
    drop(crate::write::remove_dir_all(folder));
}

/// A marker unique to this test process and `case`.
pub(crate) fn mark(case: u8) -> SessionMark {
    let mut bytes = [0_u8; 16];
    bytes[..4].copy_from_slice(&std::process::id().to_le_bytes());
    bytes[4] = case;
    bytes[15] = 0x5e;
    SessionMark::from_random(bytes)
}

/// Starts `/bin/sh -c '<line> <wait for .up, at most 10 s>; exit 0' sh <folder>` with only
/// `PATH`, the writer and `mark` in its environment and waits for it to exit (its group is then
/// killed, as for any agent).
pub(crate) fn run_agent(line: &str, folder: &Path, mark: &SessionMark) {
    let path = std::env::var_os("PATH").unwrap_or_else(|| OsString::from("/usr/bin:/bin"));
    let env: BTreeMap<OsString, OsString> = [
        (OsString::from("PATH"), path),
        (OsString::from(WRITER_VAR), OsString::from(WRITER)),
        mark.var(),
    ]
    .into_iter()
    .collect();
    let spec = LaunchSpec::new(
        "/bin/sh".into(),
        vec![
            "-c".into(),
            format!(
                "{line} i=0; while [ ! -e \"$1/.up\" ] && [ $i -lt 200 ]; do sleep 0.05; \
                 i=$((i+1)); done; exit 0"
            )
            .into(),
            "sh".into(),
            folder.as_os_str().to_owned(),
        ],
        env,
        folder.to_owned(),
    )
    .unwrap();
    let Launched {
        stdin,
        stderr,
        mut handle,
        ..
    } = spawn_in_tree(&spec, &SpawnOptions::new().clear_env()).unwrap();
    drop(stdin);
    let report = block_on(async {
        let errs = async {
            drop(stderr.drain().await);
        };
        futures::future::join(Box::pin(handle.exited()), errs)
            .await
            .0
    });
    assert_eq!(report.code(), Some(0), "the agent's own exit");
    assert!(folder.join(".up").exists(), "the writer never ran");
}

/// Runs `line` as the agent of a session in `folder`, sweeps, waits [`WAIT`], and says whether
/// `late.txt` was written and how many processes the sweep ended.
pub(crate) fn written_after_sweep(line: &str, folder: &Path, mark: &SessionMark) -> (bool, usize) {
    run_agent(line, folder, mark);
    let swept = block_on(mark.sweep(&AsyncIoTimer));
    let SessionSwept::Swept { ended, left } = swept else {
        panic!("the sweep did not run: {swept:?}");
    };
    assert!(!left, "a marked process was left");
    block_on(Timer::after(WAIT));
    (folder.join("late.txt").exists(), ended)
}

/// Shape `case` of [`shapes`] in a matter folder called `name`: no `late.txt` once the session
/// is swept, and nothing carries its marker after; returns how many the sweep ended.
pub(crate) fn ends_with_the_session(case: usize, name: &str) -> usize {
    let (shape, line) = shapes().into_iter().nth(case).unwrap();
    let folder = matter(&format!("ends-{case}"), name);
    let mark = mark(u8::try_from(case).unwrap());
    let (late, ended) = written_after_sweep(&line, &folder, &mark);
    assert!(
        !late,
        "{shape}: late.txt was written after the session ended"
    );
    assert_eq!(
        block_on(mark.sweep(&AsyncIoTimer)),
        SessionSwept::Swept {
            ended: 0,
            left: false
        },
        "{shape}: a process still carries the marker"
    );
    remove(folder.parent().unwrap());
    ended
}
