//! The jail tests' world: a temp folder laid out like a data dir, two matters, a login home and
//! a Claude store, the policy for matter `m1`, and helpers that run a program through the
//! launcher and collect what it printed. The one place these tests change files: workspace
//! `clippy.toml` bans std's writes, and the expect below covers only this file.

#![expect(
    clippy::disallowed_methods,
    reason = "the jail tests' file setup calls std's writes here alone; clippy.toml bans them elsewhere"
)]

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};

use clauseo_process::{LaunchSpec, Launched, Launcher, PassthroughLauncher, SpawnOptions};
use clauseo_sandbox::{Denial, DenialSink, Policy, PolicyBuilder, SeatbeltLauncher};
use futures::io::AsyncReadExt;

static NEXT: AtomicU64 = AtomicU64::new(0);

/// A temp folder for one test, under `/private/tmp` so Unix socket paths stay under the
/// 104-byte limit. Removed on drop.
pub(crate) struct World {
    root: PathBuf,
}

impl World {
    /// The layout every jail test uses:
    ///
    /// - `data/`: the data dir stand-in, hidden; in it `agent-state/m1/codex` (the state
    ///   folder), `agent-state/m1/notes`, `agent-state/m2/...` (another matter's), and
    ///   `versions/` (the version store stand-in).
    /// - `m1/` (this matter) and `m2/` (a sibling matter, hidden).
    /// - `home/` (the home folder stand-in), `login/` (a login home: the credential link's
    ///   shape), `claude/` (a Claude store: Claude's shape), `outside/` (secrets).
    pub(crate) fn new() -> World {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let root = PathBuf::from(format!("/private/tmp/csb-{}-{n}", std::process::id()));
        if root.exists() {
            std::fs::remove_dir_all(&root).unwrap();
        }
        let w = World { root };
        for dir in [
            "data/agent-state/m1/codex",
            "data/agent-state/m1/notes",
            "data/agent-state/m2/codex",
            "data/agent-state/m2/notes",
            "data/versions",
            "m1",
            "m2",
            "home",
            "login",
            "claude",
            "outside",
        ] {
            w.dir(dir);
        }
        w.file("data/versions/v1.txt", "version store\n");
        w.file("data/agent-state/m2/codex/history.txt", "other state\n");
        w.file("data/agent-state/m2/notes/n.txt", "other notes\n");
        w.file("m2/brief.txt", "sibling matter\n");
        w.file("home/secret.txt", "home secret\n");
        w.file("login/auth.json", "{\"token\":\"stand-in\"}\n");
        w.file("login/other.txt", "login other\n");
        w.file("claude/.credentials.json", "{\"stand-in\":true}\n");
        w.file("claude/other.txt", "claude other\n");
        w.file("outside/secret.txt", "outside secret\n");
        // The credential link: a hard link in the state folder (a symbolic one would be
        // checked at its target, which the jail refuses).
        w.hard_link("login/auth.json", "data/agent-state/m1/codex/auth.json");
        w
    }

    pub(crate) fn at(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }

    pub(crate) fn dir(&self, rel: &str) -> PathBuf {
        let path = self.at(rel);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    pub(crate) fn file(&self, rel: &str, content: &str) -> PathBuf {
        let path = self.at(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, content).unwrap();
        path
    }

    pub(crate) fn hard_link(&self, original: &str, link: &str) -> PathBuf {
        let path = self.at(link);
        std::fs::hard_link(self.at(original), &path).unwrap();
        path
    }

    pub(crate) fn remove(&self, rel: &str) {
        std::fs::remove_file(self.at(rel)).unwrap();
    }

    pub(crate) fn symlink(&self, target: &str, link: &str) -> PathBuf {
        let path = self.at(link);
        std::os::unix::fs::symlink(self.at(target), &path).unwrap();
        path
    }

    /// A Unix socket listening at `rel`, accepting in a thread of its own.
    pub(crate) fn listen_unix(&self, rel: &str) -> PathBuf {
        let path = self.at(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                drop(stream);
            }
        });
        path
    }

    /// The policy for `m1`: its state folder and notes, the credential link, the Claude store,
    /// the data dir and the sibling matter hidden, and `extra` on top.
    pub(crate) fn builder(&self) -> PolicyBuilder {
        Policy::builder(self.at("m1"), self.at("data/agent-state/m1/codex"))
            .home(self.at("home"))
            .notes(self.at("data/agent-state/m1/notes"))
            .credential_link(self.at("data/agent-state/m1/codex/auth.json"))
            .claude_store(self.at("claude"))
            .hidden(self.at("data"))
            .hidden(self.at("m2"))
    }
}

impl Drop for World {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.root) {
            eprintln!("removing {}: {error}", self.root.display());
        }
    }
}

/// A sink that sends each denial down a channel.
pub(crate) struct Collect(Mutex<mpsc::Sender<Denial>>);

impl DenialSink for Collect {
    fn denied(&self, d: Denial) {
        drop(self.0.lock().unwrap().send(d));
    }
}

/// A launcher for `policy` whose denials arrive on the returned receiver.
pub(crate) fn launcher(policy: Policy) -> (SeatbeltLauncher, mpsc::Receiver<Denial>) {
    let (tx, rx) = mpsc::channel();
    let sink = Arc::new(Collect(Mutex::new(tx)));
    (SeatbeltLauncher::new(policy, SpawnOptions::new(), sink), rx)
}

/// What a program printed and how it ended.
#[derive(Debug)]
pub(crate) struct Output {
    pub(crate) code: Option<i32>,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
}

impl Output {
    #[track_caller]
    pub(crate) fn assert_ok(&self) -> &Output {
        assert_eq!(self.code, Some(0), "{self:#?}");
        self
    }

    /// Exited non-zero and said the OS refused it.
    #[track_caller]
    pub(crate) fn assert_refused(&self) -> &Output {
        assert_ne!(self.code, Some(0), "{self:#?}");
        assert!(
            self.stderr.contains("Operation not permitted"),
            "expected a refusal: {self:#?}"
        );
        self
    }
}

/// Runs `program` with `args` in `cwd` through `launcher`, with stdin closed, and waits for it.
pub(crate) fn run(
    launcher: &dyn Launcher,
    program: &str,
    args: &[&str],
    cwd: &Path,
    env: &[(&str, &str)],
) -> Output {
    let env: BTreeMap<OsString, OsString> = env
        .iter()
        .map(|(k, v)| (OsString::from(k), OsString::from(v)))
        .collect();
    let args = args.iter().map(OsString::from).collect();
    let spec = LaunchSpec::new(PathBuf::from(program), args, env, cwd.to_owned()).unwrap();
    let Launched {
        stdin,
        mut stdout,
        stderr,
        mut handle,
        ..
    } = launcher.launch(&spec).unwrap();
    drop(stdin);
    let lines = Arc::new(Mutex::new(String::new()));
    let sink = Arc::clone(&lines);
    async_io::block_on(async {
        let mut out = Vec::new();
        let read = stdout.read_to_end(&mut out);
        let pumped = stderr.lines(move |line| {
            let mut all = sink.lock().unwrap();
            all.push_str(line);
            all.push('\n');
        });
        let (read, pumped, report) = futures::join!(read, pumped, handle.exited());
        read.unwrap();
        pumped.unwrap();
        Output {
            code: report.code(),
            stdout: String::from_utf8_lossy(&out).into_owned(),
            stderr: lines.lock().unwrap().clone(),
        }
    })
}

/// Runs `program` outside the jail (the test's own set-up: git, xcode-select).
pub(crate) fn run_outside(program: &str, args: &[&str], cwd: &Path) -> Output {
    let launcher = PassthroughLauncher::new(SpawnOptions::new());
    run(&launcher, program, args, cwd, &[])
}

/// The variables that point git at another repository. A spawned process inherits the test's
/// environment, and git sets `GIT_DIR` for its hooks, so a test run from a pre-push hook would
/// otherwise run these git commands against the repository being pushed.
const GIT_REDIRECTS: [&str; 9] = [
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_COMMON_DIR",
    "GIT_NAMESPACE",
    "GIT_CEILING_DIRECTORIES",
    "GIT_PREFIX",
];

/// The `PATH` the jail's shell and git get: the jail inherits no variable, and the test
/// process's own `PATH` would make the tests depend on the machine.
const PATH: (&str, &str) = ("PATH", "/usr/bin:/bin");

/// `/usr/bin/git args` through `launcher` in `cwd`, started by `/usr/bin/env` with every
/// variable in [`GIT_REDIRECTS`] removed, so git works on the repository in `cwd` only. `env`
/// plus [`PATH`].
pub(crate) fn git(
    launcher: &dyn Launcher,
    args: &[&str],
    cwd: &Path,
    env: &[(&str, &str)],
) -> Output {
    let mut all = Vec::new();
    for name in GIT_REDIRECTS {
        all.extend(["-u", name]);
    }
    all.push("/usr/bin/git");
    all.extend_from_slice(args);
    let mut env = env.to_vec();
    env.push(PATH);
    run(launcher, "/usr/bin/env", &all, cwd, &env)
}

/// [`git`] outside the jail.
pub(crate) fn git_outside(args: &[&str], cwd: &Path) -> Output {
    let launcher = PassthroughLauncher::new(SpawnOptions::new());
    git(&launcher, args, cwd, &[])
}

/// `/bin/sh -c script` through `launcher` in `cwd`, with `args` as `$1`, `$2`, ..., and
/// [`PATH`].
pub(crate) fn sh(launcher: &dyn Launcher, cwd: &Path, script: &str, args: &[&str]) -> Output {
    let mut all = vec!["-c", script, "sh"];
    all.extend_from_slice(args);
    run(launcher, "/bin/sh", &all, cwd, &[PATH])
}

/// Every file under `dir` with its bytes, by path.
pub(crate) fn tree(dir: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut out = BTreeMap::new();
    let mut stack = vec![dir.to_owned()];
    while let Some(folder) = stack.pop() {
        for entry in std::fs::read_dir(&folder).unwrap() {
            let entry = entry.unwrap();
            let kind = entry.file_type().unwrap();
            if kind.is_dir() {
                stack.push(entry.path());
            } else {
                out.insert(
                    entry.path(),
                    std::fs::read(entry.path()).unwrap_or_default(),
                );
            }
        }
    }
    out
}
