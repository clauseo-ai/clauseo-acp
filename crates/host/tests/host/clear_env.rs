//! What reaches an agent's real process when the launcher clears the environment, as the
//! desktop's does: the host starts from a base environment holding every variable no agent may
//! get (`HostConfig::with_inherited_env`, so this process's environment is never changed), and
//! each child writes its environment with `/usr/bin/env` before it becomes `fake_agent`. A
//! `/bin/sh` wrapper does that, so these run on Unix only.

#![cfg(unix)]

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use clauseo_process::{LaunchSpec, Launched, Launcher, PassthroughLauncher, SpawnOptions};
use clauseo_protocol::{StopReason, StreamId};

use crate::codex::{CODEX, run_turn};
use crate::support::{Dir, FAKE, Scenario, matter_with_thread};

/// Inherited variables no agent gets.
const DROPPED: [(&str, &str); 7] = [
    ("SSH_CONNECTION", "10.0.0.1 50000 10.0.0.2 22"),
    ("SSH_TTY", "/dev/pts/7"),
    ("SSH_AUTH_SOCK", "/tmp/ssh-agent.sock"),
    ("NO_BROWSER", "1"),
    ("CLAUDE_CODE_REMOTE", "true"),
    ("GIT_DIR", "/elsewhere/.git"),
    ("GIT_WORK_TREE", "/elsewhere"),
];

/// Codex's dropped names (`agent/codex.rs`'s `DROPPED_VARS`).
const CODEX_DROPPED: [&str; 4] = [
    "CODEX_PATH",
    "APP_SERVER_LOGS",
    "DEFAULT_AUTH_REQUEST",
    "MODEL_PROVIDER",
];

/// An inherited `CODEX_HOME`, which the Codex agent's own replaces.
const INHERITED_CODEX_HOME: &str = "/elsewhere/codex-home";

/// An inherited `CLAUDE_CONFIG_DIR`. A Claude agent gets Clauseo's own instead (`agent/home.rs`,
/// `agent_homes.rs`); an agent of no kind, which does not read it, gets it as it is.
const INHERITED_CLAUDE_CONFIG_DIR: &str = "/elsewhere/claude";

/// What `/bin/sh` itself may add to the environment it runs `env` in.
const SHELL_OWN: [&str; 4] = ["PWD", "OLDPWD", "SHLVL", "_"];

/// One launch as the launcher saw it: the spec's environment, and the file its child writes.
type Launch = (BTreeMap<OsString, OsString>, PathBuf);

/// One launch: the spec the host built, and the environment its process saw.
struct Seen {
    spec: BTreeMap<OsString, OsString>,
    child: BTreeMap<String, String>,
}

/// The desktop's kind of launcher (`SpawnOptions::clear_env`) with each agent wrapped in
/// `/bin/sh -c '/usr/bin/env > <file>; exec "$0" "$@"'`: same variables, same folder, same
/// program and arguments once the shell execs.
struct EnvWriting {
    inner: PassthroughLauncher,
    folder: PathBuf,
    launches: Arc<Mutex<Vec<Launch>>>,
}

impl Launcher for EnvWriting {
    fn launch(&self, spec: &LaunchSpec) -> Result<Launched, clauseo_process::Error> {
        let mut launches = self.launches.lock().unwrap();
        let out = self.folder.join(format!("env-{}.txt", launches.len()));
        launches.push((spec.env().clone(), out.clone()));
        let script = format!("/usr/bin/env > '{}'; exec \"$0\" \"$@\"", out.display());
        let args = [OsString::from("-c"), OsString::from(script)]
            .into_iter()
            .chain(std::iter::once(spec.program().as_os_str().to_owned()))
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

/// The variables `env` printed, by name. The base environment holds no value with a newline.
fn read_env(path: &std::path::Path) -> BTreeMap<String, String> {
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .filter_map(|line| line.split_once('='))
        .map(|(name, value)| (name.to_owned(), value.to_owned()))
        .collect()
}

/// Runs one turn on a Codex thread, then one on a `fake-agent` thread, with the host starting
/// from `base`; returns what each launch saw, Codex's first.
fn launch_both(dir: &Dir, base: Vec<(OsString, OsString)>) -> Vec<Seen> {
    let launches = Arc::new(Mutex::new(Vec::new()));
    let launcher = EnvWriting {
        inner: PassthroughLauncher::new(SpawnOptions::new().clear_env()),
        folder: dir.work().to_owned(),
        launches: Arc::clone(&launches),
    };
    let host = dir.start_launching(0, Arc::new(launcher), |config| {
        config.with_inherited_env(base)
    });
    let (matter, codex_thread, _) =
        matter_with_thread(dir, &host, "m", &[("brief.txt", "x\n")], CODEX);
    for thread in [codex_thread, host.create_thread(matter, FAKE)] {
        let mut events = host.events(StreamId::Thread(thread));
        let turn = run_turn(&host, &mut events, thread);
        events.assert_no_mismatch();
        assert_eq!(events.turn_ended(turn).unwrap().stop, StopReason::EndTurn);
    }
    host.shutdown();
    let launches = launches.lock().unwrap();
    launches
        .iter()
        .map(|(spec, out)| Seen {
            spec: spec.clone(),
            child: read_env(out),
        })
        .collect()
}

/// Every dropped variable stays out of the real child, an inherited `CODEX_HOME` gives way to
/// the Codex agent's own, `PATH`, `HOME` and the spec's variables arrive, and nothing of this
/// test process's own environment does: the child's variables are the spec's, plus what the
/// wrapping shell adds.
#[test]
fn the_child_gets_the_spec_and_nothing_dropped() {
    let dir = Dir::new(
        "clear_env_child",
        &[
            (CODEX, Scenario::Bundled("codex-handshake")),
            (FAKE, Scenario::Bundled("prompt-stream")),
        ],
    );
    let path = std::env::var("PATH").unwrap();
    let home = dir.data().join("home").display().to_string();
    let mut base: Vec<(String, String)> = vec![
        ("PATH".to_owned(), path.clone()),
        ("HOME".to_owned(), home.clone()),
        ("CLAUSEO_PROBE".to_owned(), "1".to_owned()),
        ("CODEX_HOME".to_owned(), INHERITED_CODEX_HOME.to_owned()),
        (
            "CLAUDE_CONFIG_DIR".to_owned(),
            INHERITED_CLAUDE_CONFIG_DIR.to_owned(),
        ),
    ];
    base.extend(DROPPED.map(|(name, value)| (name.to_owned(), value.to_owned())));
    base.extend(CODEX_DROPPED.map(|name| (name.to_owned(), "inherited".to_owned())));
    let seen = launch_both(
        &dir,
        base.into_iter()
            .map(|(name, value)| (OsString::from(name), OsString::from(value)))
            .collect(),
    );
    let [codex, fake] = seen.as_slice() else {
        panic!("two launches, not {}", seen.len());
    };

    for (agent, seen) in [("codex", codex), ("fake", fake)] {
        for (name, _) in DROPPED {
            assert_eq!(seen.child.get(name), None, "{name} reached {agent}");
        }
        assert_eq!(seen.child.get("PATH"), Some(&path), "{agent}");
        assert_eq!(seen.child.get("HOME"), Some(&home), "{agent}");
        assert_eq!(
            seen.child.get("CLAUSEO_PROBE").map(String::as_str),
            Some("1"),
            "{agent}"
        );
        let spec: BTreeSet<String> = seen
            .spec
            .keys()
            .map(|name| name.to_str().unwrap().to_owned())
            .collect();
        let child: BTreeSet<String> = seen
            .child
            .keys()
            .filter(|name| !SHELL_OWN.contains(&name.as_str()))
            .cloned()
            .collect();
        assert_eq!(child, spec, "{agent} got other variables than its spec's");
        for (name, value) in &seen.spec {
            let name = name.to_str().unwrap();
            assert_eq!(
                seen.child.get(name).map(String::as_str),
                value.to_str(),
                "{agent}'s {name}"
            );
        }
    }

    for name in CODEX_DROPPED {
        assert_eq!(codex.child.get(name), None, "{name} reached Codex");
    }
    let codex_home = dir.data().join("agents").join(CODEX).join("home");
    assert_eq!(
        codex.child.get("CODEX_HOME"),
        Some(&codex_home.display().to_string())
    );
    // An agent of no kind is not Claude Code: the host forces no config folder on it.
    assert_eq!(
        fake.child.get("CLAUDE_CONFIG_DIR").map(String::as_str),
        Some(INHERITED_CLAUDE_CONFIG_DIR)
    );
}
