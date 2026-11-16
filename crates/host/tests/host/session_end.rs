//! A session's leftover processes end with the session. The agent, before it becomes
//! `fake_agent`, starts a writer in five shapes: `setsid` in a subshell that exits, a double
//! fork, `nohup`, fork-`setsid`-fork from a shell of its own, and `nohup` from a shell in a
//! session of its own that exits. Each waits for the test's go, given only once the host has
//! stopped, then writes its number into `late.txt` in the matter folder. The host's stop ends
//! them all: no `late.txt`. They inherit the launch's `CLAUSEO_SESSION`, which is what the
//! host's sweep finds them by.

#![cfg(any(target_os = "linux", target_os = "macos"))]

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use clauseo_process::{LaunchSpec, Launched, Launcher, PassthroughLauncher};
use clauseo_protocol::StreamId;

use crate::codex::run_turn;
use crate::support::{Dir, FAKE, Scenario, matter_with_thread, wait};

/// How long the test waits after its go for a writer that is still alive to write.
const AFTER_GO: Duration = Duration::from_secs(2);

/// The writer, run as `sh <writer> <folder> <go> <n>`: it marks `.up-<n>` once it runs, waits
/// for the go file (at most 60 s), then appends `n` to `late.txt`.
const WRITER: &str = "touch \"$1/.up-$3\"; i=0; while [ ! -e \"$2\" ] && [ $i -lt 600 ]; do \
                      sleep 0.1; i=$((i+1)); done; echo \"$3\" >> \"$1/late.txt\"\n";

/// The number of shapes in [`writers`].
const SHAPES: u8 = 5;

/// What starts a command in a session of its own (see `process/tests/session_sweep.rs`).
#[cfg(target_os = "linux")]
const SETSID: &str = "setsid";
#[cfg(target_os = "macos")]
const SETSID: &str = "perl -MPOSIX -e \"POSIX::setsid() or die; exec @ARGV\" --";

/// The agent's writers, as `/bin/sh` with the matter folder as `$1`, the go file as `$2` and the
/// writer script as `$3`; the agent goes on once every writer has marked (at most 10 s).
fn writers() -> String {
    let quiet = "</dev/null >/dev/null 2>&1";
    let writer = |n: u8| format!("sh \"$3\" \"$1\" \"$2\" {n} {quiet}");
    let up: Vec<String> = (1..=SHAPES)
        .map(|n| format!("[ -e \"$1/.up-{n}\" ]"))
        .collect();
    format!(
        "({SETSID} {one} &); ({two} &) & nohup {three} & \
         sh -c '( {SETSID} {four} & ) &' sh \"$1\" \"$2\" \"$3\" {quiet}; \
         {SETSID} sh -c 'nohup {five} & exit 0' sh \"$1\" \"$2\" \"$3\" {quiet} & \
         i=0; while [ $i -lt 200 ] && ! {{ {up}; }}; do sleep 0.05; i=$((i+1)); done; ",
        one = writer(1),
        two = writer(2),
        three = writer(3),
        four = writer(4),
        five = writer(5),
        up = up.join(" && "),
    )
}

/// Wraps each agent in `/bin/sh -c '<writers> shift 3; exec "$0" "$@"' <program> <folder>
/// <go> <writer> <args>`, as a sandbox launcher rewrites a spec: the same environment and
/// folder, and the same program and arguments once the shell execs.
struct Writing {
    inner: PassthroughLauncher,
    go: PathBuf,
    writer: PathBuf,
    launches: Arc<Mutex<usize>>,
}

impl Launcher for Writing {
    fn launch(&self, spec: &LaunchSpec) -> Result<Launched, clauseo_process::Error> {
        *self.launches.lock().unwrap() += 1;
        let script = format!("{} shift 3; exec \"$0\" \"$@\"", writers());
        let args = [
            OsString::from("-c"),
            OsString::from(script),
            spec.program().as_os_str().to_owned(),
            spec.cwd().as_os_str().to_owned(),
            self.go.as_os_str().to_owned(),
            self.writer.as_os_str().to_owned(),
        ]
        .into_iter()
        .chain(spec.args().iter().cloned())
        .collect();
        let wrapped = LaunchSpec::new(
            PathBuf::from("/bin/sh"),
            args,
            spec.env().clone(),
            spec.cwd().to_owned(),
        )?;
        self.inner.launch(&wrapped)
    }
}

/// Writes `text` to `path`: the writer script, then the go file.
fn put(path: &Path, text: &str) {
    crate::write::write(path, text.as_bytes());
}

#[test]
fn a_sessions_leftover_processes_end_with_the_session() {
    let dir = Dir::new(
        "session_end_leftovers",
        &[(FAKE, Scenario::Bundled("prompt-stream"))],
    );
    let go_file = dir.work().join("go");
    let writer = dir.work().join("writer.sh");
    put(&writer, WRITER);
    let launches = Arc::new(Mutex::new(0));
    let launcher = Writing {
        inner: dir.launcher(),
        go: go_file.clone(),
        writer,
        launches: Arc::clone(&launches),
    };
    let host = dir.start_launching(0, Arc::new(launcher), |config| config);
    let (_, thread, folder) =
        matter_with_thread(&dir, &host, "O'Brien & Sons", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    run_turn(&host, &mut events, thread);
    events.assert_no_mismatch();
    assert_eq!(*launches.lock().unwrap(), 1, "one launch");
    for n in 1..=SHAPES {
        let up = folder.join(format!(".up-{n}"));
        assert!(up.exists(), "writer {n} never ran");
    }

    // The session ends with the host's stop; only then may a writer that still runs write.
    host.shutdown();
    put(&go_file, "go");
    wait("a writer's late write", async_io::Timer::after(AFTER_GO));
    let late = std::fs::read_to_string(folder.join("late.txt")).unwrap_or_default();
    assert_eq!(late, "", "writers wrote late.txt after the session ended");
}
