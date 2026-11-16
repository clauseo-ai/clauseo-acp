//! The exact `_meta.claudeCode.options` a Claude launch emits when the launch profile's hook
//! has `ListAgents` and `ListPeers` in its own `allowedTools` (over the real profile's hook:
//! Clauseo's skills plugin and one firm skill pack), and both plugin folders, written to
//! `CLAUSEO_EMIT_DIR` for a run against Claude Code. Ignored: it writes files outside the temp
//! folder, and only when that variable is set.

use std::path::{Path, PathBuf};

use clauseo_profile::{BrowserSidecars, ClauseoProfile, ProfilePaths};
use clauseo_protocol::AgentKind;
use clauseo_settings::Locations;
use serde_json::{Map, Value, json};

use super::browser::Switch;
use super::profile::{ClaudeKeys, Folders, Login, fill_meta};
use super::sandbox::{ClaudeSandbox, SandboxOs, SandboxPaths};
use super::{GuardHook, McpControl, session_meta};
use crate::agent::data_fence::{self, LaunchDeny};
use crate::browser::seed_sites::SEED_SITES;

/// The folder the files go to.
const EMIT_VAR: &str = "CLAUSEO_EMIT_DIR";
/// The firm pack's id.
const PACK_ID: &str = "nda-kit";
/// The firm pack's one skill.
const PACK_SKILL: &str = "nda-review";
/// The run folder's session id.
const SESSION: &str = "t-1";
/// What the hook's own `allowedTools` gains: two names the launch leaves out of `tools`.
const HOOK_ALLOWED: [&str; 2] = ["ListAgents", "ListPeers"];
/// The built-in plugin's name (its manifest's).
const CLAUSEO_PLUGIN: &str = "clauseo";
/// The pack plugin's name (its manifest's).
const PACKS_PLUGIN: &str = "clauseo-packs";

/// One launch's folders under a temp root, and the hook the real profile gave for them.
struct Rig {
    root: PathBuf,
    data: PathBuf,
    matter: PathBuf,
    program: PathBuf,
    claude_tmp: PathBuf,
    run: PathBuf,
    notes: PathBuf,
    plugin: PathBuf,
    packs_plugin: PathBuf,
    /// Claude's home, `<data>/agents/claude-acp/home`.
    home: PathBuf,
    /// The data folder deny set the launch prep builds for these folders ([`app_deny`]).
    deny: LaunchDeny,
    /// The name of Claude's project folder for the matter (`data_fence::claude_projects`).
    project: String,
    hook: Map<String, Value>,
}

impl Rig {
    /// The profile under `<root>/data`, its skills plugin written, the firm pack put in its
    /// packs folder and written into the launch's own pack plugin, as the launch prep does;
    /// Claude's home and the run's work, temp and notes folders made, and the deny set built.
    /// `root` is taken canonical, so the data folder has one spelling and every path in the
    /// output has its placeholder.
    fn new(root: PathBuf) -> Self {
        let root = dunce::canonicalize(&root).unwrap();
        let data = root.join("data");
        let app = root.join("app");
        let program = app.join("clauseo");
        let paths = ProfilePaths::new(
            &Locations::under_root(&data),
            program.clone(),
            app.join("stetdoc"),
            BrowserSidecars {
                clauseo_mcp: app.join("clauseo-mcp"),
                agent_browser: app.join("agent-browser"),
            },
        )
        .unwrap();
        let (profile, refused) = ClauseoProfile::new(paths, None).with_builtin_skills(None);
        assert!(refused.is_empty(), "{refused:?}");
        for write in profile.launch_writes(Some(AgentKind::Claude)) {
            clauseo_profile::write_skills(&write).unwrap();
        }
        let packs_root = profile.paths().skill_packs().to_path_buf();
        put_pack(&packs_root);
        let pack = clauseo_profile::packs::inspect(&packs_root, PACK_ID, false).unwrap();
        let run = data.join("run").join(SESSION);
        let packs_plugin = run.join("skill-packs");
        let written = clauseo_profile::write_packs(&packs_plugin, &[pack]).unwrap();
        let mut hook = profile
            .session_meta_with_packs(Some(AgentKind::Claude), Some(&written))
            .unwrap();
        allow_in_hook(&mut hook, &HOOK_ALLOWED);
        let matter = root.join("matter");
        let notes = data.join("agent-state").join("m-1").join("notes");
        let home = Locations::under_root(&data).agent_home("claude-acp");
        for folder in [&matter, &notes, &home, &run.join("work"), &run.join("tmp")] {
            crate::write::create_dir_all(folder).unwrap();
        }
        let deny = app_deny(&data, &run, &notes, &matter, &home);
        let cwd = crate::agent::session_folder(&matter);
        let project = data_fence::claude_projects(&home, &[&cwd, &matter])
            .first()
            .and_then(|folder| folder.file_name())
            .and_then(|name| name.to_str())
            .unwrap()
            .to_owned();
        Self {
            plugin: profile.paths().claude_plugin().to_path_buf(),
            notes,
            home,
            deny,
            project,
            claude_tmp: root.join("clsb-emit"),
            matter,
            program,
            run,
            data,
            packs_plugin,
            hook,
            root,
        }
    }

    /// A Claude launch's `_meta.claudeCode.options`, its paths rewritten: the macOS sandbox
    /// when `sandboxed`, none otherwise (Windows), and the guard's hook either way.
    fn launch(&self, sandboxed: bool, forced_ask: bool) -> Value {
        let work = self.run.join("work");
        let tmp = self.run.join("tmp");
        let downloads = self.run.join("agent-browser").join("downloads");
        let skills = [self.plugin.join("skills")];
        let data = [self.data.clone()];
        let paths = SandboxPaths {
            matter: &self.matter,
            work: &work,
            tmp: &tmp,
            claude_tmp: Some(self.claude_tmp.as_path()),
            downloads: &downloads,
            skill_packs: &self.packs_plugin,
            notes: &self.notes,
            skills: &skills,
            path: &[],
            env_file: None,
            ca_file: None,
            claude_home: &self.home,
            data: &data,
            other_matters: &[],
        };
        let sandbox = ClaudeSandbox::for_os(SandboxOs::Mac, &paths).unwrap();
        let folders = Folders::of(&work, &self.notes).unwrap();
        let keys = ClaudeKeys {
            web_search: Switch {
                on: true,
                locked: false,
            },
            login: Login::FirstParty,
            folders,
            seed_sites: SEED_SITES,
            forced_ask,
            data: &self.deny,
            case_law: false,
            browser: false,
            sandbox: sandboxed.then_some(&sandbox),
            matter: Some(self.matter.as_path()),
            trusted_servers: &[],
        };
        let guard = GuardHook {
            program: &self.program,
            matter: &self.matter,
            ask_all: forced_ask,
        };
        let mut meta = session_meta(
            Some(self.hook.clone()),
            McpControl::Clauseo,
            Some(guard),
            Some(&keys),
        );
        fill_meta(&mut meta, folders).unwrap();
        let mut out = meta["claudeCode"]["options"].clone();
        rewrite(&mut out, &self.swaps());
        out
    }

    /// Each absolute path's placeholder, the most specific first.
    fn swaps(&self) -> Vec<(String, String)> {
        vec![
            (self.project.clone(), "{{PROJECT}}".to_owned()),
            (
                text(&self.plugin),
                format!("{{{{PLUGINS}}}}/{CLAUSEO_PLUGIN}"),
            ),
            (
                text(&self.packs_plugin),
                format!("{{{{PLUGINS}}}}/{PACKS_PLUGIN}"),
            ),
            (text(&self.matter), "{{MATTER}}".to_owned()),
            (text(&self.program), "{{GUARD}}".to_owned()),
            (text(&self.claude_tmp), "{{CLAUDE_TMP}}".to_owned()),
            (text(&self.data), "{{DATA}}".to_owned()),
            (text(&self.root), "{{ROOT}}".to_owned()),
        ]
    }
}

/// The data folder deny set the launch prep builds (`launch_checks/folders.rs` `launch_deny`)
/// for a Claude launch with its run root `run`, the matter's `notes`, the `matter` folder
/// and Claude's `home`: every entry of `data` but the own folders (the run root, the notes,
/// the matter's session folder, `<home>/plans`) and the matter's project `tool-results`, and
/// the run's work and notes folders as its own reads.
fn app_deny(data: &Path, run: &Path, notes: &Path, matter: &Path, home: &Path) -> LaunchDeny {
    let cwd = crate::agent::session_folder(matter);
    let own = [
        run.to_path_buf(),
        notes.to_path_buf(),
        cwd.clone(),
        home.join("plans"),
    ];
    let projects = data_fence::claude_projects(home, &[&cwd, matter]);
    // The spellings as `launch_checks/folders.rs` `data_spellings` gives them.
    let mut spellings: Vec<PathBuf> = dunce::canonicalize(data).into_iter().collect();
    if !spellings.iter().any(|canonical| canonical == data) {
        spellings.push(data.to_owned());
    }
    let mut deny = data_fence::launch_deny(spellings, data, &own, &projects);
    deny.own_reads = data_fence::own_reads(data, &[&run.join("work"), notes]);
    deny
}

/// `path` as UTF-8 text.
fn text(path: &Path) -> String {
    path.to_str().unwrap().to_owned()
}

/// Writes `bytes` to `path`, making its folder.
fn put(path: &Path, bytes: &[u8]) {
    crate::write::create_dir_all(path.parent().unwrap()).unwrap();
    crate::write::write_file(path, bytes).unwrap();
}

/// The firm pack [`PACK_ID`] in `packs_root`, with its one skill and one reference, as the
/// host's pack tests write one (`tests/host/packs.rs` `put_pack`).
fn put_pack(packs_root: &Path) {
    let root = packs_root.join(PACK_ID);
    let manifest = json!({"format": 1, "id": PACK_ID, "name": "A pack", "version": "1.0.0",
        "author": "Example Chambers", "source_url": "https://example.org/pack",
        "licence": "CC-BY-4.0", "last_reviewed": "2026-10-20", "instruction_only": true,
        "skills": [{"name": PACK_SKILL}]});
    put(
        &root.join("clauseo-pack.json"),
        manifest.to_string().as_bytes(),
    );
    let skill = root.join("skills").join(PACK_SKILL);
    let body =
        format!("---\nname: {PACK_SKILL}\ndescription: Reviews an NDA.\n---\n# NDA review\n");
    put(&skill.join("SKILL.md"), body.as_bytes());
    put(&skill.join("references").join("notes.md"), b"Notes.\n");
}

/// Appends each of `names` to the hook's `claudeCode.options.allowedTools`, making it.
fn allow_in_hook(hook: &mut Map<String, Value>, names: &[&str]) {
    let options = hook
        .entry("claudeCode")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .unwrap()
        .entry("options")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .unwrap();
    let allowed = options
        .entry("allowedTools")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .unwrap();
    allowed.extend(names.iter().map(|name| json!(name)));
}

/// Every string in `value` with each of `swaps`' paths replaced by its placeholder, in order.
fn rewrite(value: &mut Value, swaps: &[(String, String)]) {
    match value {
        Value::String(held) => {
            for (from, to) in swaps {
                if held.contains(from.as_str()) {
                    *held = held.replace(from.as_str(), to);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                rewrite(item, swaps);
            }
        }
        Value::Object(map) => {
            for item in map.values_mut() {
                rewrite(item, swaps);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

/// Copies the folder `from` to `to`, file by file.
fn copy_tree(from: &Path, to: &Path) {
    crate::write::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            crate::write::write_file(&target, &std::fs::read(entry.path()).unwrap()).unwrap();
        }
    }
}

/// The README beside the file: what the launches are and the placeholders.
fn readme() -> String {
    let lines = [
        "# Emitted Claude launch options: a hook that allows ListAgents and ListPeers",
        "",
        "Written by `crates/host/src/agent/loads/emitted_tests.rs`",
        "(`writes_the_emitted_toollist_options`, run with `CLAUSEO_EMIT_DIR` set), from",
        "`agent::loads::session_meta` over the real launch profile's hook",
        "(`ClauseoProfile::session_meta_with_packs`) with `ListAgents` and `ListPeers` added",
        "to the hook's own `claudeCode.options.allowedTools`: Clauseo's skills plugin plus one",
        "firm skill pack, `McpControl::Clauseo`, a first-party login, web search on, no case",
        "law, no browser, forced ask off, and the pre-write guard's hook.",
        "",
        "`emitted-toollist.json`: `hook_allowedTools` (what the hook added), then the whole",
        "`_meta.claudeCode.options` for `mac` (the macOS sandbox) and `win` (no sandbox):",
        "`tools`, `allowedTools`, `disallowedTools`, `settings` (the flag settings),",
        "`settingSources`, `plugins` and the rest. `tools` follows the build's OS",
        "(`cfg!(windows)` adds Windows' shell and search tools): `built_on` names it.",
        "",
        "## Placeholders",
        "",
        "- `{{PLUGINS}}`: this folder's `plugins/` (`plugins/clauseo`, `plugins/clauseo-packs`).",
        "- `{{DATA}}`: Clauseo's data folder; `{{MATTER}}`: the matter folder; `{{GUARD}}`: the",
        "  guard program (run with `--pre-write-guard --matter <matter>`; a real build or a",
        "  stub); `{{CLAUDE_TMP}}`: Claude's short temp folder; `{{ROOT}}`: anything else under",
        "  the test's temp root; `{{PROJECT}}`: the name of Claude's project folder for the",
        "  matter.",
    ];
    lines.join("\n") + "\n"
}

/// Writes the options file, both plugin folders and the README to `CLAUSEO_EMIT_DIR`; does
/// nothing when it is unset.
#[test]
#[ignore = "writes files outside the temp folder; run with CLAUSEO_EMIT_DIR set"]
fn writes_the_emitted_toollist_options() {
    let Some(out) = std::env::var_os(EMIT_VAR) else {
        return;
    };
    let out = PathBuf::from(out);
    let root = std::env::temp_dir().join(format!("clauseo-emit-toollist-{}", std::process::id()));
    crate::write::remove_dir_all(&root).unwrap();
    crate::write::create_dir_all(&root).unwrap();
    let rig = Rig::new(root.clone());
    let emitted = json!({
        "built_on": std::env::consts::OS,
        "hook_allowedTools": HOOK_ALLOWED,
        "mac": rig.launch(true, false),
        "win": rig.launch(false, false),
    });
    assert!(
        !emitted.to_string().contains(&text(&root)),
        "a path left unrewritten"
    );
    for launch in ["mac", "win"] {
        let options = &emitted[launch];
        let listed = |key: &str| {
            options[key]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        };
        for name in HOOK_ALLOWED {
            assert!(
                listed("allowedTools").contains(&name.to_owned()),
                "{launch} {name}"
            );
            assert!(
                !listed("tools").contains(&name.to_owned()),
                "{launch} {name}"
            );
            assert!(
                listed("disallowedTools").contains(&name.to_owned()),
                "{launch} {name}"
            );
        }
    }
    crate::write::create_dir_all(&out).unwrap();
    let body = serde_json::to_string_pretty(&emitted).unwrap() + "\n";
    crate::write::write_file(&out.join("emitted-toollist.json"), body.as_bytes()).unwrap();
    for (from, name) in [
        (&rig.plugin, CLAUSEO_PLUGIN),
        (&rig.packs_plugin, PACKS_PLUGIN),
    ] {
        let to = out.join("plugins").join(name);
        crate::write::remove_dir_all(&to).unwrap();
        copy_tree(from, &to);
    }
    crate::write::write_file(&out.join("README-toollist.md"), readme().as_bytes()).unwrap();
    crate::write::remove_dir_all(&root).unwrap();
}
