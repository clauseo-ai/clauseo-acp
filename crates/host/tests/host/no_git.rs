//! The `git` that does nothing (`HostConfig::with_no_git`, `agent/no_git.rs`) on both agents'
//! launches: its folder first on the process's `PATH`, ahead of stetdoc's; a Codex session's
//! shells given that whole `PATH`; git's attribute source on both, and the repository ceiling on
//! Codex alone; the host's own `PATH` never touched. A program missing at launch (as a virus
//! scanner's quarantine leaves it) or changed refuses the launch with `agent.tool_missing`, and
//! nothing starts.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use clauseo_host::NoGit;
use clauseo_protocol::{ErrorCode, StreamId};
use serde_json::json;
use sha2::{Digest as _, Sha256};

use crate::codex::{CODEX, run_turn, turn_failed};
use crate::profile_rig::{CLAUDE, Wired, agent_lines, agents, codex_lines, settings, template};
use crate::support::{Dir, matter_with_thread};
use crate::write;

/// The bytes the tests' program holds.
const PROGRAM: &[u8] = b"the program's bytes";

/// `<data>/tools/no-git/git` holding [`PROGRAM`] under `dir`, and the [`NoGit`] expecting it.
fn installed(dir: &Dir) -> NoGit {
    let folder = dir.data().join("tools").join("no-git");
    write::create_dir_all(&folder);
    let program = folder.join(format!("git{}", std::env::consts::EXE_SUFFIX));
    write::write(&program, PROGRAM);
    NoGit::new(program, Some(Sha256::digest(PROGRAM).into()))
}

/// The value of `name` in `env`, ignoring ASCII case.
fn var<'a>(env: &'a std::collections::BTreeMap<OsString, OsString>, name: &str) -> Option<&'a str> {
    env.iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .and_then(|(_, value)| value.to_str())
}

/// The folders of a `PATH` value.
fn parts(path: &str) -> Vec<PathBuf> {
    let separator = if cfg!(windows) { ';' } else { ':' };
    path.split(separator).map(PathBuf::from).collect()
}

#[test]
fn both_agents_find_the_program_first_and_the_host_does_not() {
    let dir = Dir::new("no_git_first", &[]);
    let claude = template(
        &dir,
        "no-git",
        &agent_lines("no-git", "session/new", &json!({}), &[]),
    );
    let codex = template(
        &dir,
        "no-git-codex",
        &codex_lines("no-git-codex", &json!({})),
    );
    settings(
        &dir,
        agents(&[(CLAUDE, &claude, json!({})), (CODEX, &codex, json!({}))]),
        json!({}),
    );
    let no_git = installed(&dir);
    let folder = no_git.program().parent().unwrap().to_path_buf();
    let stetdoc = dir.data().join("stetdoc");
    let launcher = Wired::new(&dir);
    let host = dir.start_launching(0, Arc::<Wired>::clone(&launcher), |config| {
        config
            .with_agent_path_prepend(stetdoc.clone())
            .with_no_git(no_git.clone())
    });
    let matter_folder = dir.folder("m", &[("brief.txt", "x\n")]);
    let matter = host.create_matter("m", &matter_folder);
    crate::profile_rig::turn(&host, host.create_thread(matter, CLAUDE));
    crate::profile_rig::turn(&host, host.create_thread(matter, CODEX));
    host.shutdown();
    let seen = launcher.seen();
    assert_eq!(seen.len(), 2);
    for (at, launch) in seen.iter().enumerate() {
        let path = parts(var(&launch.env, "PATH").expect("a PATH"));
        assert_eq!(path.first(), Some(&folder), "launch {at}: {path:?}");
        assert_eq!(path.get(1), Some(&stetdoc), "launch {at}: {path:?}");
        assert_eq!(
            var(&launch.env, "GIT_ATTR_SOURCE"),
            Some("4b825dc642cb6eb9a060e54bf8d69288fbee4904"),
            "launch {at}"
        );
    }
    let (claude, codex) = (&seen[0], &seen[1]);
    assert_eq!(var(&claude.env, "GIT_CEILING_DIRECTORIES"), None);
    let parent = matter_folder.parent().unwrap();
    let ceiling = var(&codex.env, "GIT_CEILING_DIRECTORIES").expect("the ceiling");
    let same = |path: &Path| dunce::canonicalize(path).unwrap();
    assert_eq!(same(Path::new(ceiling)), same(parent));
    let config = codex.codex_config().expect("a CODEX_CONFIG");
    let shell = config["shell_environment_policy"]["set"]["PATH"]
        .as_str()
        .expect("the shells' PATH");
    assert_eq!(Some(shell), var(&codex.env, "PATH"));
    assert_eq!(parts(shell).first(), Some(&folder));
    // The host's own process never gets the folder.
    let own = std::env::var_os("PATH").unwrap_or_default();
    assert!(!parts(&own.to_string_lossy()).contains(&folder));
}

#[test]
fn a_vanished_or_changed_program_refuses_both_agents_before_anything_starts() {
    for (label, agent) in [("claude", CLAUDE), ("codex", CODEX)] {
        for (case, spoil) in [
            (
                "vanished",
                (|program: &Path| write::remove_file(program)) as fn(&Path),
            ),
            ("changed", |program: &Path| {
                write::write(program, b"other bytes");
            }),
        ] {
            let dir = Dir::new(&format!("no_git_{case}_{label}"), &[]);
            let script = template(&dir, "never", &codex_lines("never", &json!({})));
            settings(&dir, agents(&[(agent, &script, json!({}))]), json!({}));
            let no_git = installed(&dir);
            spoil(no_git.program());
            let launcher = Wired::new(&dir);
            let host = dir.start_launching(0, Arc::<Wired>::clone(&launcher), |config| {
                config.with_no_git(no_git.clone())
            });
            let (_, thread, _) =
                matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], agent);
            let mut events = host.events(StreamId::Thread(thread));
            let turn = run_turn(&host, &mut events, thread);
            let failed = turn_failed(&events, turn)
                .unwrap_or_else(|| panic!("{case} {label}: no TurnFailed: {}", events.dump()));
            assert_eq!(failed.code, ErrorCode::AgentToolMissing, "{case} {label}");
            assert!(
                launcher.seen().is_empty(),
                "{case} {label}: an agent started"
            );
            host.shutdown();
        }
    }
}

#[test]
fn a_program_gone_mid_session_refuses_the_next_prompt() {
    for agent in [CLAUDE, CODEX] {
        let dir = Dir::new(&format!("no_git_mid_{agent}"), &[]);
        let script = if agent == CLAUDE {
            template(
                &dir,
                "mid",
                &agent_lines("mid", "session/new", &json!({}), &[]),
            )
        } else {
            template(&dir, "mid", &codex_lines("mid", &json!({})))
        };
        settings(&dir, agents(&[(agent, &script, json!({}))]), json!({}));
        let no_git = installed(&dir);
        let launcher = Wired::new(&dir);
        let host = dir.start_launching(0, Arc::<Wired>::clone(&launcher), |config| {
            config.with_no_git(no_git.clone())
        });
        let matter = host.create_matter("m", &dir.folder("m", &[("brief.txt", "x\n")]));
        let thread = host.create_thread(matter, agent);
        crate::profile_rig::turn(&host, thread);
        // What a virus scanner's quarantine leaves mid-session: nothing at the name.
        write::remove_file(no_git.program());
        let mut events = host.events(StreamId::Thread(thread));
        let turn = run_turn(&host, &mut events, thread);
        let failed = turn_failed(&events, turn)
            .unwrap_or_else(|| panic!("{agent}: no TurnFailed: {}", events.dump()));
        assert_eq!(failed.code, ErrorCode::AgentToolMissing, "{agent}");
        host.shutdown();
    }
}

/// A launcher that removes the program as it starts the agent, after the launch prep's check
/// and before the session call: as a virus scanner's quarantine could.
struct RemovesAtLaunch {
    inner: Arc<Wired>,
    program: PathBuf,
}

impl clauseo_process::Launcher for RemovesAtLaunch {
    fn launch(
        &self,
        spec: &clauseo_process::LaunchSpec,
    ) -> Result<clauseo_process::Launched, clauseo_process::Error> {
        if self.program.exists() {
            write::remove_file(&self.program);
        }
        self.inner.launch(spec)
    }
}

/// The first three lines of a scenario (its header, `initialize` and the answer): an agent that
/// takes no session call, so a `session/new` reaching it is a mismatch (exit 3).
fn initialize_only(lines: &str) -> String {
    lines.lines().take(3).collect::<Vec<_>>().join("\n")
}

#[test]
fn a_program_gone_between_launch_and_session_call_refuses_the_session() {
    for agent in [CLAUDE, CODEX] {
        let dir = Dir::new(&format!("no_git_session_{agent}"), &[]);
        let lines = if agent == CLAUDE {
            agent_lines("session", "session/new", &json!({}), &[])
        } else {
            codex_lines("session", &json!({}))
        };
        let script = template(&dir, "session", &initialize_only(&lines));
        settings(&dir, agents(&[(agent, &script, json!({}))]), json!({}));
        let no_git = installed(&dir);
        let launcher = Arc::new(RemovesAtLaunch {
            inner: Wired::new(&dir),
            program: no_git.program().to_path_buf(),
        });
        let host = dir.start_launching(0, launcher, |config| config.with_no_git(no_git.clone()));
        let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], agent);
        let mut events = host.events(StreamId::Thread(thread));
        let turn = run_turn(&host, &mut events, thread);
        let failed = turn_failed(&events, turn)
            .unwrap_or_else(|| panic!("{agent}: no TurnFailed: {}", events.dump()));
        assert_eq!(failed.code, ErrorCode::AgentToolMissing, "{agent}");
        // The session call's own re-check refused it: the agent got no `session/new`, which it
        // would answer by exiting 3 (a mismatch), failing the turn with another code.
        events.assert_no_mismatch();
        host.shutdown();
    }
}
