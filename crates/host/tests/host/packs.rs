//! Skill packs at launch (decision record 0040, PR-D), through the host's interface with real
//! packs on disk: a launch gets its matter's packs as a plugin of its own under its run folder,
//! named second in Claude's `_meta` and listed in Codex's index; a pack that changed since it was
//! turned on is left out with one notice per thread session. The log line is in `packs_log`, the
//! timing run in `packs_timing`. The request's rules (the matter's list, the policy) are unit
//! tests beside the code (`launch_checks/packs/tests.rs`).

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use clauseo_host::{PackRequest, ProfileHook};
use clauseo_profile::{
    BrowserSidecars, ClauseoProfile, PackWrite, PacksWritten, ProfilePaths, SkillWrite,
    SkillsWritten,
};
use clauseo_protocol::{
    AgentId, AgentKind, DomainEvent, FieldChange, HostCommand, MatterId, NoticeLevel, PackId,
    StreamId, ThreadId,
};
use clauseo_settings::Locations;
use serde_json::{Map, Value, json};

use crate::matters::{created, import, packs};
use crate::observe::Events;
use crate::profile_rig::{
    CLAUDE, CODEX, Seen, Wired, agent_lines, agents, codex_lines, files_under, path_text, settings,
    template, turn,
};
use crate::support::{Dir, Running, Scenario};
use crate::write;

/// The desktop adapter's mapping (`crates/desktop/src/profile.rs`) over a real profile.
pub(crate) struct PackHook(pub(crate) Arc<ClauseoProfile>);

impl ProfileHook for PackHook {
    fn session_meta(
        &self,
        agent: &AgentId,
        packs: Option<&PacksWritten>,
    ) -> Option<Map<String, Value>> {
        self.0.session_meta_with_packs(AgentKind::of(agent), packs)
    }

    fn launch_env(&self, _agent: &AgentId) -> BTreeMap<OsString, OsString> {
        BTreeMap::new()
    }

    fn codex_config(
        &self,
        agent: &AgentId,
        written: Option<&SkillsWritten>,
        packs: Option<&PacksWritten>,
    ) -> Map<String, Value> {
        self.0
            .codex_keys_with_packs(AgentKind::of(agent), written, packs)
    }

    fn launch_writes(&self, agent: &AgentId) -> Vec<SkillWrite> {
        self.0.launch_writes(AgentKind::of(agent))
    }

    fn pack_write(&self, agent: &AgentId, request: &PackRequest) -> Option<PackWrite> {
        self.0.pack_write(
            AgentKind::of(agent),
            request.entries().to_vec(),
            request.allow_scripts(),
        )
    }
}

/// The profile under `root` with R22's built-ins: its packs folder is `<root>/skill-packs`.
pub(crate) fn profile(root: &Path) -> ClauseoProfile {
    let paths = ProfilePaths::new(
        &Locations::under_root(root),
        root.join("app").join("clauseo"),
        root.join("stetdoc").join("stetdoc"),
        BrowserSidecars {
            clauseo_mcp: root.join("app").join("clauseo-mcp"),
            agent_browser: root.join("app").join("agent-browser"),
        },
    )
    .unwrap();
    let (profile, refused) = ClauseoProfile::new(paths, None).with_builtin_skills(None);
    assert!(refused.is_empty(), "{refused:?}");
    profile
}

/// `<data>/skill-packs/<id>`.
pub(crate) fn pack_dir(dir: &Dir, id: &str) -> PathBuf {
    dir.data().join("skill-packs").join(id)
}

/// Writes the pack `id` with `skills`, each a `SKILL.md` and one reference of `reference`
/// bytes, and returns its hash as `skills.enabled` records it.
pub(crate) fn put_pack(dir: &Dir, id: &str, skills: &[&str], reference: &[u8]) -> String {
    let root = pack_dir(dir, id);
    let listed: Vec<Value> = skills.iter().map(|name| json!({"name": name})).collect();
    let manifest = json!({"format": 1, "id": id, "name": "A pack", "version": "1.0.0",
        "author": "Example Chambers", "source_url": "https://example.org/pack",
        "licence": "CC-BY-4.0", "last_reviewed": "2026-10-20", "instruction_only": true,
        "skills": listed});
    write::write(
        &root.join("clauseo-pack.json"),
        manifest.to_string().as_bytes(),
    );
    for name in skills {
        let skill = root.join("skills").join(name);
        let text = format!("---\nname: {name}\ndescription: Reviews a {name}.\n---\n# {name}\n");
        write::write(&skill.join("SKILL.md"), text.as_bytes());
        write::write(&skill.join("references").join("notes.md"), reference);
    }
    hash_of(dir, id)
}

/// The pack `id`'s hash now.
pub(crate) fn hash_of(dir: &Dir, id: &str) -> String {
    clauseo_profile::packs::inspect(&dir.data().join("skill-packs"), id, false)
        .unwrap()
        .hash()
        .to_string()
}

/// The `skills` section: `extra` `any` and `enabled`.
pub(crate) fn skills_on(enabled: &[(&str, &str)]) -> Value {
    let enabled: Map<String, Value> = enabled
        .iter()
        .map(|(id, hash)| ((*id).to_owned(), json!(hash)))
        .collect();
    json!({"skills": {"extra": "any", "enabled": enabled}})
}

/// A Claude scenario whose `session/new` names `plugins` (`{{any}}` for the built-in one,
/// `@PACKS@` for the launch's pack plugin).
fn claude_with(dir: &Dir, name: &str, with_packs: bool) -> Scenario {
    let mut plugins = vec![json!({"type": "local", "path": "{{any}}"})];
    if with_packs {
        plugins.push(json!({"type": "local", "path": "@PACKS@"}));
    }
    let params = json!({"_meta": {"claudeCode": {"options": {"plugins": plugins}}}});
    template(dir, name, &agent_lines(name, "session/new", &params, &[]))
}

/// A Claude scenario that starts a session (new, or a load of `sess-1`) and exits on the
/// prompt, so each prompt launches the agent again.
pub(crate) fn exits_on_prompt(dir: &Dir, name: &str) -> Scenario {
    let modes = json!({"currentModeId": "default", "availableModes": [
        {"id": "default", "name": "Manual"}, {"id": "acceptEdits", "name": "Accept edits"},
        {"id": "plan", "name": "Plan"}, {"id": "auto", "name": "Auto"}]});
    let lines = [
        json!({"scenario": name, "description": "exits on each prompt", "on_stdin_close": "exit"}),
        json!({"expect": {"method": "initialize", "as": "init", "params": {"protocolVersion": 1}}}),
        json!({"respond": {"to": "init", "result": {"protocolVersion": 1,
            "agentCapabilities": {"loadSession": true}, "authMethods": [],
            "agentInfo": {"name": "fake_agent", "version": "0.0.0"}}}}),
        json!({"one_of": [
            [{"expect": {"method": "session/new", "as": "new", "params": {}}},
             {"respond": {"to": "new", "result": {"sessionId": "sess-1", "modes": modes}}}],
            [{"expect": {"method": "session/load", "as": "load", "params": {"sessionId": "sess-1"}}},
             {"respond": {"to": "load", "result": {"modes": modes}}}]]}),
        json!({"expect": {"method": "session/prompt", "as": "p", "params": {}}}),
        json!({"exit": {"code": 1}}),
    ]
    .map(|line| line.to_string())
    .join("\n");
    template(dir, name, &lines)
}

/// Starts the host with [`PackHook`] over the profile under the data folder, launching
/// through `launcher`.
pub(crate) fn start(dir: &Dir, run: u16, launcher: &Arc<Wired>) -> Running {
    let hook = PackHook(Arc::new(profile(dir.data())));
    dir.start_launching(run, Arc::<Wired>::clone(launcher), |config| {
        config.with_profile_hook(Box::new(hook))
    })
}

/// Sets `matter`'s pack list to `ids`.
fn bind(host: &Running, matter: MatterId, ids: &[&str]) {
    host.ok(HostCommand::UpdateMatter {
        matter,
        client: FieldChange::Keep,
        next_date: FieldChange::Keep,
        skill_packs: FieldChange::Set(packs(ids)),
        pinned: None,
    });
}

/// The warning notices `events` holds.
pub(crate) fn warnings(events: &Events) -> Vec<String> {
    events
        .events()
        .filter_map(|event| match event {
            DomainEvent::Notice(notice) if notice.level == NoticeLevel::Warning => {
                Some(notice.text.clone())
            }
            _ => None,
        })
        .collect()
}

/// One prompt on `thread`, until its turn ends, however it ended.
pub(crate) fn prompt_until_over(host: &Running, thread: ThreadId) -> Events {
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Go.");
    events.until("the turn is over", |events| {
        events.turn_ended(turn).is_some()
    });
    events
}

/// The skills a launch's plugin held, by folder name.
fn skill_folders(seen: &Seen) -> Vec<String> {
    let mut names: Vec<String> = seen
        .packs
        .iter()
        .filter_map(|file| file.strip_prefix("skills/"))
        .filter_map(|rest| rest.split('/').next())
        .map(str::to_owned)
        .collect();
    names.dedup();
    names
}

#[test]
fn a_claude_launch_gets_the_matter_packs_as_a_second_plugin() {
    let dir = Dir::new("packs_claude", &[]);
    let hash = put_pack(&dir, "nda-kit", &["nda-review", "nda-redline"], b"Notes.\n");
    let claude = claude_with(&dir, "packs-claude", true);
    let entries = agents(&[(CLAUDE, &claude, json!({}))]);
    settings(&dir, entries, skills_on(&[("nda-kit", &hash)]));
    let launcher = Wired::new(&dir);
    let host = start(&dir, 0, &launcher);
    let matter = host.create_matter("m", &dir.folder("m", &[]));
    turn(&host, host.create_thread(matter, CLAUDE));
    host.shutdown();
    let seen = launcher.seen();
    assert_eq!(seen.len(), 1);
    assert_eq!(
        skill_folders(&seen[0]),
        ["nda-kit--nda-redline", "nda-kit--nda-review"]
    );
    assert!(
        seen[0]
            .packs
            .contains(&".claude-plugin/plugin.json".to_owned()),
        "{:?}",
        seen[0].packs
    );
}

#[test]
fn two_matters_running_at_once_see_only_their_own_packs() {
    let dir = Dir::new("packs_two_matters", &[]);
    let nda = put_pack(&dir, "nda-kit", &["nda-review"], b"Notes.\n");
    let lease = put_pack(&dir, "lease-kit", &["lease-review"], b"Notes.\n");
    let claude = claude_with(&dir, "packs-two", true);
    let entries = agents(&[(CLAUDE, &claude, json!({}))]);
    settings(
        &dir,
        entries,
        skills_on(&[("nda-kit", &nda), ("lease-kit", &lease)]),
    );
    let launcher = Wired::new(&dir);
    let host = start(&dir, 0, &launcher);
    let first = host.create_matter("a", &dir.folder("a", &[]));
    let second = host.create_matter("b", &dir.folder("b", &[]));
    bind(&host, first, &["nda-kit"]);
    bind(&host, second, &["lease-kit"]);
    // Both agents stay running after their turns: the second launches while the first runs.
    turn(&host, host.create_thread(first, CLAUDE));
    turn(&host, host.create_thread(second, CLAUDE));
    let seen = launcher.seen();
    assert_eq!(seen.len(), 2);
    assert_eq!(skill_folders(&seen[0]), ["nda-kit--nda-review"]);
    assert_eq!(skill_folders(&seen[1]), ["lease-kit--lease-review"]);
    // With both running, each plugin on disk still holds only its own matter's pack.
    for (launch, own) in seen
        .iter()
        .zip(["nda-kit--nda-review", "lease-kit--lease-review"])
    {
        let run = launch.run.as_ref().unwrap();
        let on_disk = files_under(&run.join("skill-packs").join("skills"));
        assert!(
            on_disk.iter().all(|file| file.starts_with(own)),
            "{on_disk:?}"
        );
    }
    host.shutdown();
}

#[test]
fn the_codex_index_names_exactly_the_pack_files_this_launch_wrote() {
    let dir = Dir::new("packs_codex_index", &[]);
    let hash = put_pack(&dir, "nda-kit", &["nda-review", "nda-redline"], b"Notes.\n");
    let codex = template(&dir, "packs-codex", &codex_lines("packs-codex", &json!({})));
    let entries = agents(&[(CODEX, &codex, json!({}))]);
    settings(&dir, entries, skills_on(&[("nda-kit", &hash)]));
    let launcher = Wired::new(&dir);
    let host = start(&dir, 0, &launcher);
    let matter = host.create_matter("m", &dir.folder("m", &[]));
    turn(&host, host.create_thread(matter, CODEX));
    turn(&host, host.create_thread(matter, CODEX));
    host.shutdown();
    let seen = launcher.seen();
    assert_eq!(seen.len(), 2);
    for (at, launch) in seen.iter().enumerate() {
        let config = launch.codex_config().expect("a CODEX_CONFIG");
        let text = config["developer_instructions"].as_str().unwrap();
        let plugin = launch.run.as_ref().unwrap().join("skill-packs");
        let wrote: Vec<String> = launch
            .packs
            .iter()
            .filter(|file| file.ends_with("/SKILL.md"))
            // Joined part by part, so the expected path uses the OS's own separator, as the index
            // does (a `/`-joined tail would mix separators on Windows).
            .map(|file| path_text(&file.split('/').fold(plugin.clone(), |at, part| at.join(part))))
            .collect();
        assert_eq!(wrote.len(), 2, "{:?}", launch.packs);
        for path in &wrote {
            assert!(text.contains(&format!("`{path}`")), "launch {at}: {text}");
        }
        // Every pack line names this launch's plugin, and nothing else does.
        let lines = text
            .lines()
            .filter(|line| line.contains("nda-kit--"))
            .count();
        assert_eq!(lines, 2, "launch {at}: {text}");
        assert_eq!(
            text.matches("skill-packs").count(),
            2,
            "launch {at}: {text}"
        );
    }
}

#[test]
fn the_pack_plugin_goes_with_the_run_folder() {
    let dir = Dir::new("packs_run_folder", &[]);
    let hash = put_pack(&dir, "nda-kit", &["nda-review"], b"Notes.\n");
    let claude = claude_with(&dir, "packs-gone", true);
    let entries = agents(&[(CLAUDE, &claude, json!({}))]);
    settings(&dir, entries, skills_on(&[("nda-kit", &hash)]));
    let launcher = Wired::new(&dir);
    let host = start(&dir, 0, &launcher);
    let matter = host.create_matter("m", &dir.folder("m", &[]));
    turn(&host, host.create_thread(matter, CLAUDE));
    let seen = launcher.seen();
    let plugin = seen[0].run.as_ref().unwrap().join("skill-packs");
    assert!(plugin.is_dir());
    host.shutdown();
    assert!(!plugin.exists(), "{}", plugin.display());
    assert!(!seen[0].run.as_ref().unwrap().exists());
}

/// The warnings `thread` holds so far.
fn thread_warnings(host: &Running, thread: ThreadId) -> Vec<String> {
    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    warnings(&events)
}

#[test]
fn the_notice_is_shown_once_per_thread() {
    let dir = Dir::new("packs_notice_once", &[]);
    let hash = put_pack(&dir, "nda-kit", &["nda-review"], b"Notes.\n");
    // Changed since it was turned on.
    write::write(&pack_dir(&dir, "nda-kit").join("README.md"), b"# Changed\n");
    let looping = exits_on_prompt(&dir, "packs-once");
    let entries = agents(&[(CLAUDE, &looping, json!({}))]);
    settings(&dir, entries, skills_on(&[("nda-kit", &hash)]));
    let launcher = Wired::new(&dir);
    let host = start(&dir, 0, &launcher);
    let matter = host.create_matter("m", &dir.folder("m", &[]));
    let first = host.create_thread(matter, CLAUDE);
    let second = host.create_thread(matter, CLAUDE);
    let changed = "Clauseo didn't load the skill pack \"nda-kit\". Its files changed after it was turned on. Turn it on again to use it.";
    // The agent exits on each prompt: three relaunches of one thread, as in a restart loop,
    // tell it once.
    for _ in 0..3 {
        prompt_until_over(&host, first);
    }
    assert_eq!(thread_warnings(&host, first), [changed]);
    // Another thread keeps its own record: it is told too.
    prompt_until_over(&host, second);
    assert_eq!(thread_warnings(&host, second), [changed]);
    // A new reason for the same pack (a file the layout does not allow) is told once more.
    write::write(&pack_dir(&dir, "nda-kit").join("extra.txt"), b"x\n");
    for _ in 0..2 {
        prompt_until_over(&host, first);
    }
    let invalid =
        "Clauseo didn't load the skill pack \"nda-kit\". Its files aren't a valid skill pack.";
    assert_eq!(thread_warnings(&host, first), [changed, invalid]);
    assert_eq!(thread_warnings(&host, second), [changed]);
    host.shutdown();
    let seen = launcher.seen();
    assert_eq!(seen.len(), 6);
    assert!(
        seen.iter().all(|launch| launch.packs.is_empty()),
        "{seen:?}"
    );
}

#[test]
fn a_restored_matter_list_still_skips_a_pack_whose_hash_no_longer_matches() {
    let dir = Dir::new("packs_restored", &[]);
    let nda = put_pack(&dir, "nda-kit", &["nda-review"], b"Notes.\n");
    let lease = put_pack(&dir, "lease-kit", &["lease-review"], b"Notes.\n");
    let claude = claude_with(&dir, "packs-restored", true);
    let entries = agents(&[(CLAUDE, &claude, json!({}))]);
    settings(
        &dir,
        entries,
        skills_on(&[("nda-kit", &nda), ("lease-kit", &lease)]),
    );
    let launcher = Wired::new(&dir);
    let host = start(&dir, 0, &launcher);
    let folder = dir.folder("Kaveri Agro", &[("brief.txt", "Clause 4\n")]);
    let matter = created(import(&host, "Kaveri Agro", &folder, (None, None)));
    bind(&host, matter, &["nda-kit", "lease-kit"]);
    host.ok(HostCommand::ForgetMatter { matter });
    // While the matter is forgotten, one of its packs changes.
    write::write(
        &pack_dir(&dir, "nda-kit").join("skills/nda-review/references/notes.md"),
        b"Changed.\n",
    );
    assert_eq!(
        created(import(&host, "Kaveri Agro", &folder, (None, None))),
        matter
    );
    let thread = host.create_thread(matter, CLAUDE);
    let events = prompt_until_over(&host, thread);
    events.assert_no_mismatch();
    assert_eq!(
        warnings(&events),
        [
            "Clauseo didn't load the skill pack \"nda-kit\". Its files changed after it was turned on. Turn it on again to use it."
        ]
    );
    host.shutdown();
    let seen = launcher.seen();
    assert_eq!(skill_folders(&seen[0]), ["lease-kit--lease-review"]);
}

/// The matter's list is the one way the main window names packs (`UpdateMatter.skill_packs`;
/// `SetSkillPack` is the settings window's), so it can only narrow what Settings turned on:
/// a listed pack left off, or one whose files changed after it was turned on, never loads.
#[test]
fn a_matter_list_never_loads_a_pack_that_is_off_or_changed() {
    let dir = Dir::new("packs_matter_narrows", &[]);
    put_pack(&dir, "a-kit", &["a-review"], b"Notes.\n");
    put_pack(&dir, "b-kit", &["b-review"], b"Notes.\n");
    put_pack(&dir, "c-kit", &["c-review"], b"Notes.\n");
    let claude = claude_with(&dir, "packs-narrows", true);
    let entries = agents(&[(CLAUDE, &claude, json!({}))]);
    settings(&dir, entries, skills_on(&[]));
    let launcher = Wired::new(&dir);
    let host = start(&dir, 0, &launcher);
    for id in ["a-kit", "c-kit"] {
        host.ok(HostCommand::SetSkillPack {
            id: PackId::new(id).unwrap(),
            on: true,
        });
    }
    // One byte of C changes after it was turned on.
    write::write(
        &pack_dir(&dir, "c-kit").join("skills/c-review/references/notes.md"),
        b"Notes!\n",
    );
    let matter = host.create_matter("m", &dir.folder("m", &[]));
    bind(&host, matter, &["a-kit", "b-kit", "c-kit"]);
    let thread = host.create_thread(matter, CLAUDE);
    let events = prompt_until_over(&host, thread);
    events.assert_no_mismatch();
    assert_eq!(
        warnings(&events),
        [
            "Clauseo didn't load the skill pack \"b-kit\". This matter is set to use it, but it isn't turned on.",
            "Clauseo didn't load the skill pack \"c-kit\". Its files changed after it was turned on. Turn it on again to use it.",
        ]
    );
    host.shutdown();
    let seen = launcher.seen();
    assert_eq!(seen.len(), 1);
    assert_eq!(skill_folders(&seen[0]), ["a-kit--a-review"]);
}
