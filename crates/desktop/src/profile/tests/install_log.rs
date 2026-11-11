//! The agent-browser copy at app start (`install_browser`): what a debug build copies, and the
//! support lines, one on the profile's lifecycle target per start, saying what happened and
//! naming no path.

use std::sync::{Arc, Mutex, PoisonError};

use clauseo_settings::Locations;

use super::super::{DesktopProfile, LIFECYCLE, Layout};
use super::{CLAUDE, CODEX, agent, developer_instructions};
use crate::test_support::Scratch;
use clauseo_host::ProfileHook as _;
use clauseo_profile::write_skills;

/// The hook built for the data folder of `locations` with `path` as `PATH`, as a debug build
/// with no bundled sidecar, on an OS where an agent gets the shell command or not (`shell`).
fn build(locations: &Locations, path: &std::path::Path, shell: bool) -> DesktopProfile {
    let layout = Layout {
        exe: std::env::current_exe().unwrap(),
        macos: false,
        resource_dir: None,
        shell_browser: shell,
    };
    DesktopProfile::build_in(locations, &layout, Some(path.as_os_str())).unwrap()
}

/// What a test's subscriber wrote.
#[derive(Clone, Default)]
struct Lines(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Lines {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Runs `run` under a subscriber that writes every line, and gives the agent-browser lines.
fn captured(run: impl FnOnce()) -> Vec<String> {
    let lines = Lines::default();
    let writer = lines.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(move || writer.clone())
        .with_ansi(false)
        .finish();
    tracing::subscriber::with_default(subscriber, run);
    let bytes = lines
        .0
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    String::from_utf8(bytes)
        .unwrap()
        .lines()
        .filter(|line| line.contains("agent-browser"))
        .map(str::to_owned)
        .collect()
}

/// host.log keeps info lines only from a `clauseo_*` crate's `::lifecycle` target.
#[test]
fn the_target_is_one_host_log_keeps() {
    assert!(LIFECYCLE.starts_with("clauseo_desktop::"));
    assert!(LIFECYCLE.ends_with("::lifecycle"));
}

/// A first start copies, a second finds the copy up to date, and a start with no program, or
/// on an OS where no agent gets the shell command, says it is left out: one info line each on
/// the lifecycle target, with no folder in it.
#[test]
fn each_start_logs_what_the_copy_did_and_no_path() {
    let scratch = Scratch::new("profile-install-log", &["data", "bin", "empty"]);
    scratch.file(
        "bin/agent-browser",
        b"#!/bin/sh\necho 'agent-browser 0.41.1'\n",
    );
    scratch.executable("bin/agent-browser");
    let bin = scratch.dir("bin");
    let empty = scratch.dir("empty");
    let locations = Locations::under_root(&scratch.dir("data"));
    let start = |path: &std::path::Path, shell: bool| {
        captured(|| {
            build(&locations, path, shell);
        })
    };
    let first = start(&bin, true);
    let second = start(&bin, true);
    let none = start(&empty, true);
    let off = start(&bin, false);
    for (lines, outcome) in [
        (&first, "copied"),
        (&second, "up_to_date"),
        (&none, "no_sidecar"),
        (&off, "off_on_this_os"),
    ] {
        assert_eq!(lines.len(), 1, "{lines:?}");
        let line = &lines[0];
        assert!(line.contains(" INFO "), "{line}");
        assert!(line.contains(LIFECYCLE), "{line}");
        assert!(line.contains(&format!("outcome=\"{outcome}\"")), "{line}");
        let root = scratch.path().to_str().unwrap();
        assert!(!line.contains(root), "{line}");
    }
}

/// A debug build with no bundled sidecar copies the first `agent-browser` on `PATH` into
/// `<data>/tools` and wires that folder, and the browser's skill comes with that copy whether
/// or not this machine has a Chrome (each session's `{browser}` says whether it has one). With no
/// program anywhere there is no browser and no skill. Where no agent gets the shell command
/// (every OS for launch, `clauseo_profile::servers::shell_browser`) nothing is copied, and there
/// is no skill either: Claude has no browser, and Codex's entry runs the bundled sidecar.
#[test]
fn build_copies_agent_browser_from_path_in_a_debug_build() {
    let scratch = Scratch::new("profile-build-browser", &["data", "bin"]);
    scratch.file(
        "bin/agent-browser",
        b"#!/bin/sh\necho 'agent-browser 0.41.1'\n",
    );
    scratch.executable("bin/agent-browser");
    let folder = scratch.dir("bin");
    let locations = Locations::under_root(&scratch.dir("data"));
    let off = build(&locations, &folder, false);
    assert_eq!(off.browser_tool(), None);
    let tools = locations.data_dir().join("tools");
    assert!(!tools.join("agent-browser").exists());
    let token = write_skills(&off.launch_writes(&agent(CLAUDE))[0]).unwrap();
    assert!(!token.root().join("agent-browser").exists());
    let hook = build(&locations, &folder, true);
    assert_eq!(hook.browser_tool(), Some(tools.clone()));
    assert_eq!(
        std::fs::read(tools.join("agent-browser")).unwrap(),
        std::fs::read(folder.join("agent-browser")).unwrap()
    );
    let token = write_skills(&hook.launch_writes(&agent(CLAUDE))[0]).unwrap();
    assert!(
        token
            .root()
            .join("agent-browser")
            .join("SKILL.md")
            .is_file()
    );
    // Codex's index never names it: Codex has the `browser` entry's tools instead.
    let token = write_skills(&hook.launch_writes(&agent(CODEX))[0]).unwrap();
    let keys = hook.codex_config(&agent(CODEX), Some(&token), None);
    let skill = token.root().join("agent-browser").join("SKILL.md");
    assert!(!developer_instructions(&keys).contains(skill.to_str().unwrap()));
    assert!(!developer_instructions(&keys).contains("agent-browser"));

    let empty = scratch.dir("data");
    let none = build(&locations, &empty, true);
    assert_eq!(none.browser_tool(), None);
    let token = write_skills(&none.launch_writes(&agent(CLAUDE))[0]).unwrap();
    assert!(!token.root().join("agent-browser").exists());
}
