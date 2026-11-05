//! The adapter against a scratch data folder: what each agent gets, the skills token's scope,
//! and the wiring into the host's config.

use std::ffi::OsString;
use std::path::PathBuf;

use clauseo_host::{McpServer, PackRequest, ProfileHook as _};
use clauseo_profile::packs;
use clauseo_profile::{ClauseoProfile, SkillWrite, SkillsWritten, write_packs, write_skills};
use clauseo_protocol::{AgentId, AgentKind};
use clauseo_settings::{Locations, SkillsExtra, SkillsSettings};
use serde_json::Value;

use super::{DesktopProfile, paths_for};
// Only the tests not built on macOS use it (`Scratch::executable` is not built there).
#[cfg(not(target_os = "macos"))]
use super::first_on_path;
use crate::test_support::Scratch;

const CLAUDE: &str = "claude-acp";
const CODEX: &str = "codex-acp";

fn agent(id: &str) -> AgentId {
    AgentId::new(id).unwrap()
}

/// A profile under `scratch`'s data folder with R22's built-ins and no stetdoc skill, stetdoc
/// at `<scratch>/bin/stetdoc/stetdoc` (never run), and the app at `<scratch>/app/clauseo`.
fn profile(scratch: &Scratch) -> ClauseoProfile {
    let locations = Locations::under_root(scratch.path());
    let exe = scratch.path().join("app").join("clauseo");
    let stetdoc = scratch.path().join("bin").join("stetdoc").join("stetdoc");
    let paths = paths_for(&locations, exe, Some(stetdoc)).unwrap();
    let (profile, refused) = ClauseoProfile::new(paths, None).with_builtin_skills(None);
    assert!(refused.is_empty(), "{refused:?}");
    profile
}

/// A real write of `profile`'s first Codex launch write: Claude's plugin.
fn written(profile: &ClauseoProfile) -> SkillsWritten {
    let writes = profile.launch_writes(Some(AgentKind::Codex));
    write_skills(&writes[0]).unwrap()
}

fn developer_instructions(keys: &serde_json::Map<String, Value>) -> String {
    keys["developer_instructions"].as_str().unwrap().to_owned()
}

#[test]
fn claude_meta_carries_the_instructions_and_the_plugin_dir() {
    let scratch = Scratch::new("profile-claude-meta", &[]);
    let profile = profile(&scratch);
    let hook = DesktopProfile::new(profile.clone(), true);
    let meta = Value::Object(hook.session_meta(&agent(CLAUDE), None).unwrap());
    assert_eq!(
        meta.pointer("/systemPrompt/append").and_then(Value::as_str),
        Some(profile.instructions().as_str())
    );
    let plugin = scratch.path().join("profile").join("claude-plugin");
    assert_eq!(
        meta.pointer("/claudeCode/options/plugins/0/path")
            .and_then(Value::as_str),
        plugin.to_str()
    );
    assert!(hook.session_meta(&agent(CODEX), None).is_none());
}

#[test]
fn codex_keys_carry_the_index_only_with_a_token_from_this_folders_write() {
    let scratch = Scratch::new("profile-codex-index", &[]);
    let profile = profile(&scratch);
    let index = profile.skills_index();
    assert_ne!(index, "");
    let hook = DesktopProfile::new(profile.clone(), true);
    let token = written(&profile);
    let keys = hook.codex_config(&agent(CODEX), Some(&token), None);
    assert!(developer_instructions(&keys).ends_with(&index));
    // Claude never gets Codex's keys, token or not.
    assert!(
        hook.codex_config(&agent(CLAUDE), Some(&token), None)
            .is_empty()
    );
}

#[test]
fn without_a_token_codex_gets_no_index() {
    let scratch = Scratch::new("profile-codex-no-token", &[]);
    let profile = profile(&scratch);
    let hook = DesktopProfile::new(profile.clone(), true);
    let keys = hook.codex_config(&agent(CODEX), None, None);
    assert_eq!(developer_instructions(&keys), profile.instructions());
    assert!(!developer_instructions(&keys).contains(&profile.skills_index()));
}

#[test]
fn two_launches_with_different_tokens_never_mix() {
    let scratch = Scratch::new("profile-tokens-mine", &[]);
    let other = Scratch::new("profile-tokens-other", &[]);
    let profile = profile(&scratch);
    let index = profile.skills_index();
    let hook = DesktopProfile::new(profile.clone(), true);
    let mine = written(&profile);
    let theirs = written(&self::profile(&other));
    // Another folder's token, then this folder's, then none, then another folder's again:
    // each launch's keys follow its own token alone, and nothing is kept between them.
    let keys = |token: Option<&SkillsWritten>| {
        developer_instructions(&hook.codex_config(&agent(CODEX), token, None))
    };
    assert!(!keys(Some(&theirs)).contains(&index));
    assert!(keys(Some(&mine)).ends_with(&index));
    assert!(!keys(None).contains(&index));
    assert!(!keys(Some(&theirs)).contains(&index));
    assert!(keys(Some(&mine)).ends_with(&index));
}

#[test]
fn codex_launch_writes_put_claudes_plugin_first() {
    let scratch = Scratch::new("profile-launch-writes", &[]);
    let profile = profile(&scratch);
    let hook = DesktopProfile::new(profile.clone(), true);
    let skills = scratch
        .path()
        .join("profile")
        .join("claude-plugin")
        .join("skills");
    for id in [CODEX, CLAUDE] {
        let writes = hook.launch_writes(&agent(id));
        assert_eq!(
            writes.first().map(SkillWrite::root),
            Some(skills.as_path()),
            "{id}"
        );
        assert_eq!(writes, profile.launch_writes(AgentKind::of(&agent(id))));
    }
}

#[test]
fn every_agent_with_a_kind_gets_the_author_and_the_browser_entry() {
    let scratch = Scratch::new("profile-env-servers", &[]);
    let hook = DesktopProfile::new(profile(&scratch), true);
    for id in [CLAUDE, CODEX] {
        let env = hook.launch_env(&agent(id));
        assert_eq!(
            env.get(&OsString::from("STETDOC_AUTHOR")),
            Some(&OsString::from("Clauseo")),
            "{id}"
        );
        let servers = hook.mcp_servers(&agent(id));
        let [
            McpServer::Stdio {
                name,
                command,
                args,
                env,
            },
        ] = servers.as_slice()
        else {
            panic!("one stdio server: {servers:?}");
        };
        assert_eq!(name, "browser");
        assert_eq!(
            command,
            &scratch
                .path()
                .join("app")
                .join(format!("clauseo-mcp{}", std::env::consts::EXE_SUFFIX))
        );
        // The release bundles it under the clauseo- prefix (.github/scripts/sidecars.mjs), since
        // the .deb and .rpm put sidecars in /usr/bin beside other programs.
        let agent_browser = scratch.path().join("app").join(format!(
            "clauseo-agent-browser{}",
            std::env::consts::EXE_SUFFIX
        ));
        let at = args
            .iter()
            .position(|arg| arg == "--agent-browser")
            .expect("--agent-browser");
        assert_eq!(args.get(at + 1).map(String::as_str), agent_browser.to_str());
        // Five variables, and the socket folder's on macOS and Linux (decision record 0030).
        assert_eq!(env.len(), if cfg!(windows) { 5 } else { 6 });
    }
}

#[test]
fn an_agent_the_profile_does_not_know_gets_nothing() {
    let scratch = Scratch::new("profile-unknown-agent", &[]);
    let profile = profile(&scratch);
    let token = written(&profile);
    let hook = DesktopProfile::new(profile, true);
    let other = agent("gemini-acp");
    assert!(hook.session_meta(&other, None).is_none());
    assert!(hook.launch_env(&other).is_empty());
    assert_eq!(hook.mcp_servers(&other), Vec::<McpServer>::new());
    assert!(hook.codex_config(&other, Some(&token), None).is_empty());
    assert!(hook.codex_config(&other, None, None).is_empty());
    assert_eq!(hook.launch_writes(&other), Vec::<SkillWrite>::new());
    assert!(hook.pack_write(&other, &request(&["nda-kit"])).is_none());
}

/// The launch's pack request with `ids` turned on (each with a made-up hash) and no matter list.
fn request(ids: &[&str]) -> PackRequest {
    let skills = SkillsSettings {
        extra: SkillsExtra::Any,
        enabled: ids
            .iter()
            .map(|id| ((*id).to_owned(), format!("sha256:{}", "1".repeat(64))))
            .collect(),
        ..SkillsSettings::default()
    };
    PackRequest::new(&skills, None)
}

#[test]
fn no_pack_file_is_read_on_the_actor() {
    let scratch = Scratch::new("profile-pack-write-pure", &[]);
    // Where the packs go is a file: a hook that read it would fail or leave the pack out.
    let packs_root = scratch.file("skill-packs", b"not a folder");
    let hook = DesktopProfile::new(profile(&scratch), true);
    let request = request(&["nda-kit"]);
    for id in [CLAUDE, CODEX] {
        let write = hook
            .pack_write(&agent(id), &request)
            .expect("packs to write");
        assert_eq!(write.packs_root(), packs_root);
        assert_eq!(write.entries(), request.entries());
        assert!(!write.allow_scripts());
    }
    assert!(
        hook.pack_write(&agent(CLAUDE), &self::request(&[]))
            .is_none()
    );
    assert_eq!(std::fs::read(&packs_root).unwrap(), b"not a folder");
}

#[test]
fn the_launch_packs_go_to_claudes_second_plugin_and_codexs_index() {
    let scratch = Scratch::new(
        "profile-pack-tokens",
        &["skill-packs/nda-kit/skills/nda-review"],
    );
    let manifest = serde_json::json!({"format": 1, "id": "nda-kit", "name": "A pack",
        "version": "1.0.0", "author": "Example Chambers", "source_url": "https://example.org/pack",
        "licence": "CC-BY-4.0", "last_reviewed": "2026-10-20", "instruction_only": true,
        "skills": [{"name": "nda-review"}]});
    scratch.file(
        "skill-packs/nda-kit/clauseo-pack.json",
        manifest.to_string().as_bytes(),
    );
    scratch.file(
        "skill-packs/nda-kit/skills/nda-review/SKILL.md",
        b"---\nname: nda-review\ndescription: Reviews an NDA.\n---\n# NDA\n",
    );
    let profile = profile(&scratch);
    let pack = packs::inspect(profile.paths().skill_packs(), "nda-kit", false).unwrap();
    let plugin = scratch.path().join("run").join("s-1").join("skill-packs");
    let token = write_packs(&plugin, &[pack]).unwrap();
    let hook = DesktopProfile::new(profile, true);
    let meta = Value::Object(hook.session_meta(&agent(CLAUDE), Some(&token)).unwrap());
    assert_eq!(
        meta.pointer("/claudeCode/options/plugins/1/path")
            .and_then(Value::as_str),
        plugin.to_str()
    );
    let keys = hook.codex_config(&agent(CODEX), None, Some(&token));
    let skill = plugin
        .join("skills")
        .join("nda-kit--nda-review")
        .join("SKILL.md");
    assert!(
        developer_instructions(&keys).contains(skill.to_str().unwrap()),
        "{keys:?}"
    );
    assert!(hook.session_meta(&agent(CODEX), Some(&token)).is_none());
}

#[test]
fn the_host_config_gets_the_hook_and_stetdocs_folder() {
    let scratch = Scratch::new("profile-host-config", &[]);
    let hook = DesktopProfile::new(profile(&scratch), true);
    let config = crate::with_profile(scratch.config(Vec::new()), Some(hook), setup_hook());
    assert!(
        config
            .profile_hook()
            .session_meta(&agent(CLAUDE), None)
            .is_some()
    );
    assert_eq!(
        config.agent_path_prepend(),
        Some(scratch.path().join("bin").join("stetdoc").as_path())
    );
}

#[test]
fn stetdoc_not_found_goes_on_no_path() {
    let scratch = Scratch::new("profile-no-stetdoc", &[]);
    let hook = DesktopProfile::new(profile(&scratch), false);
    assert_eq!(hook.path_prepend(), None);
    let config = crate::with_profile(scratch.config(Vec::new()), Some(hook), setup_hook());
    assert!(
        config
            .profile_hook()
            .session_meta(&agent(CLAUDE), None)
            .is_some()
    );
    assert_eq!(config.agent_path_prepend(), None);
}

/// A relative `PATH` folder never reaches the host: a debug build stops on it, a release build
/// skips it. An absolute one goes through unchanged.
#[test]
#[cfg_attr(
    debug_assertions,
    should_panic(expected = "the agents' PATH folder must be absolute")
)]
fn a_relative_path_folder_is_never_prepended() {
    let scratch = Scratch::new("profile-relative-prepend", &[]);
    let absolute = scratch.path().join("bin");
    assert_eq!(crate::absolute_folder(absolute.clone()), Some(absolute));
    assert_eq!(crate::absolute_folder(PathBuf::from("bin/stetdoc")), None);
}

/// Setup's hook with no key and no plan.
fn setup_hook() -> crate::setup::SetupHook {
    crate::setup::SetupHook::new(crate::setup::KeySlot::default(), std::sync::Arc::default())
}

/// With no profile the host gets setup's hook alone: no `_meta`, no `PATH` folder.
#[test]
fn no_profile_leaves_the_host_without_a_hook() {
    let scratch = Scratch::new("profile-none", &[]);
    let config = crate::with_profile(scratch.config(Vec::new()), None, setup_hook());
    assert!(
        config
            .profile_hook()
            .session_meta(&agent(CLAUDE), None)
            .is_none()
    );
    assert_eq!(config.agent_path_prepend(), None);
}

#[test]
fn build_puts_the_profile_under_the_data_folder() {
    let scratch = Scratch::new("profile-build", &[]);
    let locations = Locations::under_root(scratch.path());
    let hook = DesktopProfile::build(&locations).unwrap();
    let meta = Value::Object(hook.session_meta(&agent(CLAUDE), None).unwrap());
    let plugin = scratch.path().join("profile").join("claude-plugin");
    assert_eq!(
        meta.pointer("/claudeCode/options/plugins/0/path")
            .and_then(Value::as_str),
        plugin.to_str()
    );
}

/// A debug build finds stetdoc on `PATH`, runs it for its skill, and puts its folder first on
/// the agents' `PATH`; the skill is in Codex's index.
#[cfg(all(unix, not(target_os = "macos"), debug_assertions))]
#[test]
fn build_loads_stetdocs_skill_from_the_stetdoc_on_path() {
    let scratch = Scratch::new("profile-build-stetdoc", &["data", "tools/stetdoc"]);
    scratch.file(
        "tools/stetdoc/stetdoc",
        b"#!/bin/sh
if [ \"$1\" = --version ]; then echo 'stetdoc 9.9.9'; else
printf -- '---\\nname: stetdoc\\ndescription: Edits Word files.\\n---\\nUse it.\\n'; fi\n",
    );
    scratch.executable("tools/stetdoc/stetdoc");
    let folder = scratch.dir("tools/stetdoc");
    let locations = Locations::under_root(&scratch.dir("data"));
    let hook = DesktopProfile::build_with(&locations, Some(folder.as_os_str())).unwrap();
    assert_eq!(hook.path_prepend(), Some(folder));
    let token = write_skills(&hook.launch_writes(&agent(CODEX))[0]).unwrap();
    let keys = hook.codex_config(&agent(CODEX), Some(&token), None);
    let skill = token.root().join("stetdoc").join("SKILL.md");
    assert!(skill.is_file());
    assert!(
        developer_instructions(&keys).contains(skill.to_str().unwrap()),
        "{}",
        developer_instructions(&keys)
    );
}

/// A stub stetdoc that answers both runs at once.
#[cfg(all(unix, not(target_os = "macos"), debug_assertions))]
const STUB: &[u8] = b"#!/bin/sh\nif [ \"$1\" = --version ]; then echo 'stetdoc 9.9.9'; else\nprintf -- '---\\nname: stetdoc\\ndescription: Edits Word files.\\n---\\nUse it.\\n'; fi\n";

/// D1: a stetdoc found in a folder holding other programs (`~/.cargo/bin`) is run for its skill,
/// but its folder never goes first on the agents' PATH.
#[cfg(all(unix, not(target_os = "macos"), debug_assertions))]
#[test]
fn a_shared_path_folder_is_not_prepended() {
    let scratch = Scratch::new("profile-shared-folder", &["data", "shared"]);
    scratch.file("shared/stetdoc", STUB);
    scratch.executable("shared/stetdoc");
    scratch.file("shared/git", b"#!/bin/sh\necho evil\n");
    scratch.executable("shared/git");
    let path = std::env::join_paths([PathBuf::from("/usr/bin"), scratch.dir("shared")]).unwrap();
    let locations = Locations::under_root(&scratch.dir("data"));
    let hook = DesktopProfile::build_with(&locations, Some(&path)).unwrap();
    assert_eq!(hook.path_prepend(), None);
    let token = write_skills(&hook.launch_writes(&agent(CODEX))[0]).unwrap();
    assert!(token.root().join("stetdoc").join("SKILL.md").is_file());
}

/// D2: a `stetdoc` with no execute bit earlier on PATH is skipped, as `which` skips it, and the
/// real one later on PATH is found and loads.
#[cfg(all(unix, not(target_os = "macos"), debug_assertions))]
#[test]
fn a_non_executable_stetdoc_does_not_shadow_the_real_one() {
    let scratch = Scratch::new("profile-non-executable", &["data", "a", "b"]);
    scratch.file("a/stetdoc", b"not a program");
    let real = scratch.file("b/stetdoc", STUB);
    scratch.executable("b/stetdoc");
    let path = std::env::join_paths([scratch.dir("a"), scratch.dir("b")]).unwrap();
    assert_eq!(first_on_path("stetdoc", &path), Some(real));
    let locations = Locations::under_root(&scratch.dir("data"));
    let hook = DesktopProfile::build_with(&locations, Some(&path)).unwrap();
    assert_eq!(hook.path_prepend(), Some(scratch.dir("b")));
}

/// D3: a stetdoc that hangs on `skills get core` is stopped within `STETDOC_START_LIMIT`: the
/// build returns well before stetdoc's own 10 s limit, with no stetdoc skill and no prepend.
#[cfg(all(unix, not(target_os = "macos"), debug_assertions))]
#[test]
fn a_slow_stetdoc_is_stopped_and_treated_as_absent() {
    let scratch = Scratch::new("profile-slow-stetdoc", &["data", "slow"]);
    scratch.file(
        "slow/stetdoc",
        b"#!/bin/sh\nif [ \"$1\" = --version ]; then echo 'stetdoc 9.9.9'; else sleep 60; fi\n",
    );
    scratch.executable("slow/stetdoc");
    let path = scratch.dir("slow").into_os_string();
    let locations = Locations::under_root(&scratch.dir("data"));
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _sent = tx.send(DesktopProfile::build_with(&locations, Some(&path)));
    });
    let hook = rx
        .recv_timeout(std::time::Duration::from_secs(7))
        .expect("the build returns within 7 s")
        .unwrap();
    assert_eq!(hook.path_prepend(), None);
    let token = write_skills(&hook.launch_writes(&agent(CODEX))[0]).unwrap();
    assert!(!token.root().join("stetdoc").exists());
}

#[test]
fn a_relative_program_path_fails_the_profile() {
    let scratch = Scratch::new("profile-relative", &[]);
    let locations = Locations::under_root(scratch.path());
    let exe = scratch.path().join("app").join("clauseo");
    assert!(paths_for(&locations, exe, Some(PathBuf::from("bin/stetdoc"))).is_err());
}

/// Not on macOS, where `Scratch::executable` is not built.
#[cfg(not(target_os = "macos"))]
#[test]
fn stetdoc_is_found_on_path_only_in_an_absolute_folder() {
    let scratch = Scratch::new("profile-on-path", &["empty", "bin"]);
    let file = format!("stetdoc{}", std::env::consts::EXE_SUFFIX);
    let program = scratch.file(&format!("bin/{file}"), b"");
    #[cfg(unix)]
    scratch.executable(&format!("bin/{file}"));
    let path = std::env::join_paths([
        PathBuf::from("bin"),
        scratch.dir("empty"),
        scratch.dir("bin"),
    ])
    .unwrap();
    assert_eq!(first_on_path("stetdoc", &path), Some(program));
    let relative = std::env::join_paths([PathBuf::from("bin")]).unwrap();
    assert_eq!(first_on_path("stetdoc", &relative), None);
}
