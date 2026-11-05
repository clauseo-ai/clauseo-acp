//! Each agent with a kind launches with Clauseo's own home (`agent/home.rs`, decision record
//! 0048): Claude Code gets `<data>/agents/claude-acp/home` as `CLAUDE_CONFIG_DIR`, Codex
//! `<data>/agents/codex-acp/home` as `CODEX_HOME`, over what the host inherited, both made at the
//! launch. The profile's Codex skills go in that same Codex home, and Codex's `CODEX_CONFIG`
//! carries the two overrides `limits` reads with, from the one shared list.

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;

use clauseo_protocol::{AgentKind, StreamId};
use clauseo_settings::Locations;
use serde_json::json;

use crate::codex::run_turn;
use crate::profile_rig::{CLAUDE, CODEX, Seen, Wired};
use crate::support::{Dir, FAKE, Scenario, matter_with_thread};

/// The config folder and Codex home the host itself starts with: the lawyer's own.
const LAWYERS_CLAUDE: &str = "/elsewhere/lawyer/.claude";
const LAWYERS_CODEX: &str = "/elsewhere/lawyer/.codex";

/// Runs one turn on a Claude, a Codex and a `fake-agent` thread, with the host's environment
/// naming the lawyer's own folders; returns what each launch got, in that order.
fn launches(dir: &Dir) -> Vec<Seen> {
    let wired = Wired::new(dir);
    let mut env: Vec<(OsString, OsString)> = std::env::vars_os()
        .filter(|(name, _)| {
            !name.eq_ignore_ascii_case("CLAUDE_CONFIG_DIR")
                && !name.eq_ignore_ascii_case("CODEX_HOME")
        })
        .collect();
    env.push(("CLAUDE_CONFIG_DIR".into(), LAWYERS_CLAUDE.into()));
    env.push(("CODEX_HOME".into(), LAWYERS_CODEX.into()));
    let host = dir.start_launching(0, Arc::<Wired>::clone(&wired), |config| {
        config.with_inherited_env(env)
    });
    let (matter, claude, _) = matter_with_thread(dir, &host, "m", &[("brief.txt", "x\n")], CLAUDE);
    for thread in [
        claude,
        host.create_thread(matter, CODEX),
        host.create_thread(matter, FAKE),
    ] {
        let mut events = host.events(StreamId::Thread(thread));
        run_turn(&host, &mut events, thread);
        events.assert_no_mismatch();
    }
    host.shutdown();
    wired.seen()
}

/// `name` in `seen`'s environment, as text.
fn var(seen: &Seen, name: &str) -> Option<String> {
    seen.env
        .get(&OsString::from(name))
        .map(|value| value.to_str().unwrap().to_owned())
}

/// The folder the tests' `Locations` gives an agent of `kind` as its home.
fn home_of(dir: &Dir, kind: AgentKind) -> PathBuf {
    Locations::under_home(dir.home()).agent_home(kind.id())
}

fn three_agents(test: &str) -> Dir {
    Dir::new(
        test,
        &[
            (CLAUDE, Scenario::Bundled("claude-prompt-stream")),
            (CODEX, Scenario::Bundled("codex-handshake")),
            (FAKE, Scenario::Bundled("prompt-stream")),
        ],
    )
}

#[test]
fn claude_and_codex_launch_with_clauseos_homes_not_the_lawyers() {
    let dir = three_agents("agent_homes");
    let seen = launches(&dir);
    let [claude, codex, fake] = seen.as_slice() else {
        panic!("three launches, not {}", seen.len());
    };
    let claude_home = home_of(&dir, AgentKind::Claude);
    let codex_home = home_of(&dir, AgentKind::Codex);
    assert_eq!(
        claude_home,
        dir.data().join("agents").join(CLAUDE).join("home")
    );
    assert_eq!(
        var(claude, "CLAUDE_CONFIG_DIR"),
        Some(claude_home.display().to_string())
    );
    assert_eq!(
        var(codex, "CODEX_HOME"),
        Some(codex_home.display().to_string())
    );
    // Both homes were made, real folders under the data folder.
    for home in [&claude_home, &codex_home] {
        let meta = std::fs::symlink_metadata(home).unwrap();
        assert!(meta.is_dir(), "{}", home.display());
    }
    // An agent of no kind reads neither variable, and the host forces neither on it.
    assert_eq!(
        var(fake, "CLAUDE_CONFIG_DIR").as_deref(),
        Some(LAWYERS_CLAUDE)
    );
    assert_eq!(var(fake, "CODEX_HOME").as_deref(), Some(LAWYERS_CODEX));
}

/// The profile writes Codex's skills into `skills` of the very folder Codex is launched with as
/// `CODEX_HOME`; Claude's skills are in Clauseo's plugin, outside both homes, and passed by path.
#[test]
fn the_profiles_codex_skills_are_in_the_home_codex_launches_with() {
    let dir = three_agents("agent_homes_profile");
    let seen = launches(&dir);
    let codex = seen.get(1).expect("a Codex launch");
    let locations = Locations::under_home(dir.home());
    let skills = clauseo_profile::paths::codex_skills(&locations);
    assert_eq!(skills.file_name().unwrap(), "skills");
    assert_eq!(
        var(codex, "CODEX_HOME").map(PathBuf::from).as_deref(),
        skills.parent()
    );
    let plugin = locations.data_dir().join("profile").join("claude-plugin");
    for kind in [AgentKind::Claude, AgentKind::Codex] {
        assert!(!plugin.starts_with(home_of(&dir, kind)));
    }
}

/// R20's two Codex overrides reach the launch, from `clauseo_limits::CODEX_OVERRIDES`, the list
/// the limits reader passes as `-c` overrides too: the login stays in the home's file, and Codex's
/// shell tools keep their default exclusion of key-named variables.
#[test]
fn codex_launches_with_the_file_login_store_and_the_default_excludes() {
    for set in [
        "cli_auth_credentials_store=\"file\"",
        "shell_environment_policy.ignore_default_excludes=false",
    ] {
        assert!(clauseo_limits::CODEX_OVERRIDES.contains(&set), "{set}");
    }
    let dir = three_agents("agent_homes_overrides");
    let seen = launches(&dir);
    let config = seen
        .get(1)
        .and_then(Seen::codex_config)
        .expect("a CODEX_CONFIG");
    assert_eq!(config["cli_auth_credentials_store"], json!("file"));
    assert_eq!(
        config["shell_environment_policy"]["ignore_default_excludes"],
        json!(false)
    );
}
