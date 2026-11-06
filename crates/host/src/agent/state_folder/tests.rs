use std::path::PathBuf;

use clauseo_protocol::{AgentKind, ErrorCode, MatterId};

use super::{
    CLAUDE_REMOVED, CODEX_REMOVED, LinkError, StateLink, clean, is_other_volume, link_own,
    link_reused, linked_folders, prepare, same_file, state_dir,
};
use crate::write;

/// A fresh data folder under the temp folder for `test`, with a Codex login home holding an
/// `auth.json`; returns the data folder and the login home.
fn data(test: &str) -> (PathBuf, PathBuf) {
    let root = std::env::temp_dir().join(format!("clauseo-state-{test}-{}", std::process::id()));
    write::remove_dir_all(&root).unwrap();
    let home = root.join("agents/codex-acp/home");
    write::create_dir_all(&home).unwrap();
    write::write_file(&home.join("auth.json"), b"{\"tokens\":1}").unwrap();
    (root, home)
}

/// The `n`th matter id (a UUID v7).
fn matter(n: u32) -> MatterId {
    let mut random = [0_u8; 10];
    for (byte, from) in random.iter_mut().zip(n.to_be_bytes()) {
        *byte = from;
    }
    MatterId::new(uuid::Builder::from_unix_timestamp_millis(1_790_000_000_000, &random).into_uuid())
        .unwrap()
}

/// The state folder is `<data>/agent-state/<matter>/<agent>`, made through no link and kept;
/// a link at `agent-state` (or any part below the data folder) refuses, and nothing is made
/// where it points.
#[test]
fn the_state_folder_is_made_under_agent_state_through_no_link() {
    let (data, _home) = data("make");
    let state = state_dir(&data, matter(1), "codex-acp");
    assert_eq!(
        state,
        data.join("agent-state")
            .join(matter(1).to_string())
            .join("codex-acp")
    );
    prepare(&data, &state, AgentKind::Codex).unwrap();
    assert!(write::is_real_dir(&state));
    write::write_file(&state.join("sessions-kept"), b"x").unwrap();
    prepare(&data, &state, AgentKind::Codex).unwrap();
    assert!(state.join("sessions-kept").exists(), "the folder is kept");

    #[cfg(unix)]
    {
        let (other, _home) = self::data("make_link");
        let elsewhere = other.join("elsewhere");
        write::create_dir_all(&elsewhere).unwrap();
        write::symlink(&elsewhere, &other.join("agent-state")).unwrap();
        let state = state_dir(&other, matter(1), "codex-acp");
        let error = prepare(&other, &state, AgentKind::Codex).unwrap_err();
        assert_eq!(error.code(), ErrorCode::AgentLaunchFailed);
        assert_eq!(std::fs::read_dir(&elsewhere).unwrap().count(), 0);
        write::remove_dir_all(&other).unwrap();
    }
    write::remove_dir_all(&data).unwrap();
}

/// Every config surface on the list is removed, a folder with all it holds and a link as
/// itself (its target untouched); transcripts and anything not on the list stay.
#[test]
fn the_clean_removes_the_config_surfaces_and_keeps_the_transcripts() {
    let (data, _home) = data("clean");
    for (kind, agent, removed, kept) in [
        (
            AgentKind::Claude,
            "claude-acp",
            &CLAUDE_REMOVED[..],
            ["projects/p/s.jsonl", "todos/t.json", ".claude.json"],
        ),
        (
            AgentKind::Codex,
            "codex-acp",
            &CODEX_REMOVED[..],
            ["sessions/r.jsonl", "state_5.sqlite", "session_index.jsonl"],
        ),
    ] {
        let state = state_dir(&data, matter(2), agent);
        prepare(&data, &state, kind).unwrap();
        let target = data.join(format!("target-{agent}"));
        write::create_dir_all(&target).unwrap();
        write::write_file(&target.join("keep"), b"x").unwrap();
        for (at, name) in removed.iter().enumerate() {
            let path = state.join(name);
            match at % 3 {
                0 => write::write_file(&path, b"planted").unwrap(),
                1 => {
                    write::create_dir_all(&path.join("inner")).unwrap();
                    write::write_file(&path.join("inner/f"), b"planted").unwrap();
                }
                _ => {
                    #[cfg(unix)]
                    write::symlink(&target, &path).unwrap();
                    #[cfg(not(unix))]
                    write::write_file(&path, b"planted").unwrap();
                }
            }
        }
        for name in kept {
            let path = state.join(name);
            write::create_dir_all(path.parent().unwrap()).unwrap();
            write::write_file(&path, b"kept").unwrap();
        }
        prepare(&data, &state, kind).unwrap();
        for name in removed {
            assert!(
                std::fs::symlink_metadata(state.join(name)).is_err(),
                "{agent}: {name} stays"
            );
        }
        for name in kept {
            assert!(state.join(name).exists(), "{agent}: {name} was removed");
        }
        assert!(
            target.join("keep").exists(),
            "a link's target is never touched"
        );
    }
    write::remove_dir_all(&data).unwrap();
}

/// A clean of a missing name is no error; the lists hold no name twice and never Clauseo's own
/// files (written whole after the clean) nor the login or the transcripts.
#[test]
fn the_lists_name_no_file_clauseo_keeps() {
    for list in [&CLAUDE_REMOVED[..], &CODEX_REMOVED[..]] {
        for keep in [
            "settings.json",
            "config.toml",
            "auth.json",
            "projects",
            "sessions",
            "todos",
            "plans",
            ".claude.json",
        ] {
            assert!(!list.contains(&keep), "{keep}");
        }
        let mut sorted = list.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), list.len());
    }
    let empty = std::env::temp_dir().join(format!("clauseo-state-empty-{}", std::process::id()));
    write::create_dir_all(&empty).unwrap();
    clean(&empty, &CODEX_REMOVED).unwrap();
    write::remove_dir_all(&empty).unwrap();
}

/// On Clauseo's own login: a new matter's state folder gets a hard link to the login home's
/// `auth.json` (one file, never a copy); a second launch finds it the same; a home whose file was
/// replaced is linked again; a home with no `auth.json` (signed out) removes the matter's.
#[test]
fn the_own_login_is_hard_linked_and_relinked_when_the_ids_differ() {
    let (data, home) = data("own");
    let state = state_dir(&data, matter(3), "codex-acp");
    prepare(&data, &state, AgentKind::Codex).unwrap();
    let linked = state.join("auth.json");
    assert_eq!(link_own(&home, &state).unwrap(), StateLink::Linked);
    assert!(same_file(&home.join("auth.json"), &linked).unwrap());
    assert!(!std::fs::symlink_metadata(&linked).unwrap().is_symlink());
    assert_eq!(link_own(&home, &state).unwrap(), StateLink::Same);
    // The home's file replaced (a new sign-in): the matter's is linked again.
    write::remove_file(&home.join("auth.json")).unwrap();
    write::write_file(&home.join("auth.json"), b"{\"tokens\":2}").unwrap();
    assert!(!same_file(&home.join("auth.json"), &linked).unwrap());
    assert_eq!(link_own(&home, &state).unwrap(), StateLink::Linked);
    assert!(same_file(&home.join("auth.json"), &linked).unwrap());
    assert!(!state.join(super::RELINK_NAME).exists());
    // Signed out: the matter keeps no login.
    write::remove_file(&home.join("auth.json")).unwrap();
    assert_eq!(link_own(&home, &state).unwrap(), StateLink::NoLogin);
    assert!(std::fs::symlink_metadata(&linked).is_err());
    write::remove_dir_all(&data).unwrap();
}

/// While reused: a new matter is linked to the home's file (the lawyer's, through the home's
/// link); a matter's `auth.json` that is not that file is a split and is never re-linked; a home
/// with no login file is a split too.
#[test]
fn a_reused_login_links_a_new_matter_and_never_relinks_a_split() {
    let (data, home) = data("reused");
    let state = state_dir(&data, matter(4), "codex-acp");
    prepare(&data, &state, AgentKind::Codex).unwrap();
    assert_eq!(link_reused(&home, &state).unwrap(), StateLink::Linked);
    assert!(same_file(&home.join("auth.json"), &state.join("auth.json")).unwrap());
    assert_eq!(link_reused(&home, &state).unwrap(), StateLink::Same);
    // The home's file replaced: the matter's link is no longer it, and stays as it is.
    write::remove_file(&home.join("auth.json")).unwrap();
    write::write_file(&home.join("auth.json"), b"{\"tokens\":3}").unwrap();
    assert_eq!(link_reused(&home, &state).unwrap(), StateLink::Split);
    assert_eq!(
        std::fs::read(state.join("auth.json")).unwrap(),
        b"{\"tokens\":1}"
    );
    // The source gone: a split, nothing linked.
    let fresh = state_dir(&data, matter(5), "codex-acp");
    prepare(&data, &fresh, AgentKind::Codex).unwrap();
    write::remove_file(&home.join("auth.json")).unwrap();
    assert_eq!(link_reused(&home, &fresh).unwrap(), StateLink::Split);
    assert!(std::fs::symlink_metadata(fresh.join("auth.json")).is_err());
    write::remove_dir_all(&data).unwrap();
}

/// A symbolic link is never the login: as the home's file it is refused (own) or a split
/// (reused), and as the matter's it is a split, never followed.
#[cfg(unix)]
#[test]
fn a_symbolic_link_is_never_linked_or_taken_for_the_login() {
    let (data, home) = data("symlink");
    let real = data.join("real-auth.json");
    write::write_file(&real, b"{}").unwrap();
    write::remove_file(&home.join("auth.json")).unwrap();
    write::symlink(&real, &home.join("auth.json")).unwrap();
    let state = state_dir(&data, matter(6), "codex-acp");
    prepare(&data, &state, AgentKind::Codex).unwrap();
    assert!(link_own(&home, &state).is_err());
    assert_eq!(link_reused(&home, &state).unwrap(), StateLink::Split);
    assert!(std::fs::symlink_metadata(state.join("auth.json")).is_err());
    write::remove_dir_all(&data).unwrap();
}

/// A link across volumes (`EXDEV`, `ERROR_NOT_SAME_DEVICE`) refuses the launch with
/// `signin.reuse_other_volume`, which may not be retried; any other failure is
/// `agent.launch_failed`. Neither message names a path.
#[test]
fn a_link_across_volumes_refuses_with_its_own_code() {
    let crossing = std::io::Error::from(std::io::ErrorKind::CrossesDevices);
    assert!(is_other_volume(&crossing));
    #[cfg(unix)]
    assert!(is_other_volume(&std::io::Error::from_raw_os_error(18)));
    let refusal = LinkError::of(&crossing).refusal();
    assert_eq!(refusal.code(), ErrorCode::SigninReuseOtherVolume);
    assert!(!refusal.code().is_retryable());
    let other = LinkError::of(&std::io::Error::from(std::io::ErrorKind::PermissionDenied));
    assert_eq!(other.refusal().code(), ErrorCode::AgentLaunchFailed);
}

/// `remove_login_links`' set: every matter's Codex state folder that is a real folder, none for
/// Claude, none through a link at `agent-state` or at a matter's folder.
#[test]
fn the_linked_folders_are_every_matters_codex_state_folder() {
    let (data, home) = data("linked");
    let mut made = Vec::new();
    for n in [7, 8] {
        let state = state_dir(&data, matter(n), "codex-acp");
        prepare(&data, &state, AgentKind::Codex).unwrap();
        link_own(&home, &state).unwrap();
        made.push(state);
    }
    let claude = state_dir(&data, matter(9), "claude-acp");
    prepare(&data, &claude, AgentKind::Claude).unwrap();
    made.sort();
    assert_eq!(linked_folders(&data, "codex-acp"), made);
    assert_eq!(linked_folders(&data, "claude-acp"), Vec::<PathBuf>::new());
    #[cfg(unix)]
    {
        let elsewhere = data.join("elsewhere/codex-acp");
        write::create_dir_all(&elsewhere).unwrap();
        let link = data.join("agent-state").join(matter(10).to_string());
        write::symlink(&data.join("elsewhere"), &link).unwrap();
        assert_eq!(
            linked_folders(&data, "codex-acp"),
            made,
            "a link is not followed"
        );
    }
    write::remove_dir_all(&data).unwrap();
}

/// The first launch's folder setup for one matter, timed by hand (ignored):
///
/// ```text
/// cargo nextest run --release -p clauseo-host --run-ignored only \
///     -E 'test(timing_first_launch_state_folder)' --no-capture
/// ```
///
/// Codex: the folder made below the data folder, cleaned, `config.toml` written and the login
/// hard-linked; Claude: made, cleaned and `settings.json` written. Target: under 50 ms each.
#[test]
#[ignore = "a timing run, by hand"]
fn timing_first_launch_state_folder() {
    use crate::clock::Clock as _;
    const RUNS: u32 = 50;
    let clock = crate::clock::SystemClock;
    let now = || clock.now().mono;
    let (data, home) = data("timing");
    let claude_home = data.join("agents/claude-acp/home");
    write::create_dir_all(&claude_home).unwrap();
    let mut codex = Vec::new();
    let mut claude = Vec::new();
    for n in 0..RUNS {
        let state = state_dir(&data, matter(100 + n), "codex-acp");
        let start = now();
        prepare(&data, &state, AgentKind::Codex).unwrap();
        crate::agent::codex::write_home_config(&state).unwrap();
        link_own(&home, &state).unwrap();
        codex.push(now().saturating_sub(start));
        let state = state_dir(&data, matter(100 + n), "claude-acp");
        let start = now();
        prepare(&data, &state, AgentKind::Claude).unwrap();
        crate::agent::home::write_state_settings(&state).unwrap();
        claude.push(now().saturating_sub(start));
    }
    for (who, times) in [("codex", &mut codex), ("claude", &mut claude)] {
        times.sort();
        let median = times[times.len() / 2];
        let max = times[times.len() - 1];
        println!("{who}: first launch setup median {median:?}, max {max:?} over {RUNS} matters");
        assert!(max < std::time::Duration::from_millis(50), "{who}: {max:?}");
    }
    write::remove_dir_all(&data).unwrap();
}
