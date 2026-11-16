//! The exact flag settings a Claude launch emits over the real launch profile's hook (Clauseo's
//! skills plugin and one firm skill pack), and both plugin folders, written to
//! `CLAUSEO_EMIT_DIR` for a run against Claude Code 2.1.293. Ignored: it writes files outside
//! the temp folder, and only when that variable is set.

use std::path::{Path, PathBuf};

use clauseo_profile::{BrowserSidecars, ClauseoProfile, ProfilePaths};
use clauseo_protocol::AgentKind;
use clauseo_settings::Locations;
use serde_json::{Map, Value, json};

use super::browser::Switch;
use super::profile::{ClaudeKeys, Folders, Login, fill_meta};
use super::sandbox::{ClaudeSandbox, SandboxOs, SandboxPaths};
use super::{GuardHook, McpControl, session_meta};
use crate::agent::data_fence::LaunchDeny;
use crate::browser::seed_sites::SEED_SITES;

/// The folder the files go to.
const EMIT_VAR: &str = "CLAUSEO_EMIT_DIR";
/// The firm pack's id.
const PACK_ID: &str = "nda-kit";
/// The firm pack's one skill.
const PACK_SKILL: &str = "nda-review";
/// The run folder's session id.
const SESSION: &str = "t-1";
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
    hook: Map<String, Value>,
}

impl Rig {
    /// The profile under `<root>/data`, its skills plugin written, the firm pack put in its
    /// packs folder and written into the launch's own pack plugin, as the launch prep does.
    fn new(root: PathBuf) -> Self {
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
        let hook = profile
            .session_meta_with_packs(Some(AgentKind::Claude), Some(&written))
            .unwrap();
        let matter = root.join("matter");
        crate::write::create_dir_all(&matter).unwrap();
        Self {
            plugin: profile.paths().claude_plugin().to_path_buf(),
            notes: data.join("agent-state").join("m-1").join("notes"),
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

    /// A Claude launch's `_meta`, shaped as [`shaped`] says: the macOS sandbox when
    /// `sandboxed`, none otherwise (Windows), and the guard's hook either way.
    fn launch(&self, sandboxed: bool, forced_ask: bool) -> Value {
        let work = self.run.join("work");
        let tmp = self.run.join("tmp");
        let downloads = self.run.join("agent-browser").join("downloads");
        let skills = [self.plugin.join("skills")];
        let data = [self.data.clone()];
        let home = self.data.join("agents").join("claude-acp").join("home");
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
            claude_home: &home,
            data: &data,
            other_matters: &[],
        };
        let sandbox = ClaudeSandbox::for_os(SandboxOs::Mac, &paths).unwrap();
        let deny = LaunchDeny::default();
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
            data: &deny,
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
        let mut out = shaped(&meta);
        rewrite(&mut out, &self.swaps());
        out
    }

    /// Each absolute path's placeholder, the most specific first.
    fn swaps(&self) -> Vec<(String, String)> {
        vec![
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

/// `settingSources`, `plugins` and `settings` from `meta`'s `claudeCode.options`, every other
/// options key under `extra`, and `_meta.systemPrompt` (not an options key) beside them.
fn shaped(meta: &Map<String, Value>) -> Value {
    let mut options = meta["claudeCode"]["options"].as_object().cloned().unwrap();
    let sources = options.remove("settingSources").unwrap_or(Value::Null);
    let plugins = options.remove("plugins").unwrap_or(Value::Null);
    let settings = options.remove("settings").unwrap_or(Value::Null);
    let prompt = meta.get("systemPrompt").cloned().unwrap_or(Value::Null);
    json!({
        "settingSources": sources,
        "plugins": plugins,
        "settings": settings,
        "extra": options,
        "systemPrompt": prompt,
    })
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

/// The README beside the files: the `plugins` and `settingSources` values, the placeholders,
/// and what a Linux build leaves out.
fn readme(plugins: &Value, sources: &Value) -> String {
    let lines = [
        "# Emitted Claude flag settings",
        "",
        "Written by `crates/host/src/agent/loads/emitted_tests.rs`",
        "(`writes_the_emitted_flag_settings`, run with `CLAUSEO_EMIT_DIR` set), from",
        "`agent::loads::session_meta` over the real launch profile's hook",
        "(`ClauseoProfile::session_meta_with_packs`): Clauseo's skills plugin plus one firm",
        "skill pack (`nda-kit`, skill `nda-review`), `McpControl::Clauseo`, a first-party",
        "login, web search on, no case law, no browser, and the pre-write guard's hook.",
        "",
        "- `emitted-mac.json`: the macOS sandbox (`ClaudeSandbox::for_os(SandboxOs::Mac)`)",
        "  and the guard; `forced_ask_off` (guard `ask_all: false`) and `forced_ask_on`",
        "  (forced ask, guard `ask_all: true`).",
        "- `emitted-win.json`: the same two with no sandbox; the guard's hook is what",
        "  Windows rests on.",
        "",
        "Each shape is `{settingSources, plugins, settings, extra, systemPrompt}`: the first",
        "four from `_meta.claudeCode.options` (`extra` is every other options key:",
        "`strictMcpConfig`, `env`, `allowedTools`, `additionalDirectories`, `agents`, ...),",
        "`systemPrompt` from `_meta.systemPrompt`.",
        "",
        "## Beside `settings`",
        "",
        "`plugins` (each `{\"type\": \"local\", \"path\": ...}`, built-in first, packs second):",
        "",
        "```json",
        "@PLUGINS@",
        "```",
        "",
        "`settingSources`: `@SOURCES@`",
        "",
        "## Placeholders",
        "",
        "- `{{PLUGINS}}`: this folder's `plugins/` (`plugins/clauseo`, `plugins/clauseo-packs`,",
        "  copied from what the test wrote).",
        "- `{{DATA}}`: Clauseo's data folder; `{{MATTER}}`: the matter folder (the agent's",
        "  working folder); `{{GUARD}}`: the guard program (the app executable, run with",
        "  `--pre-write-guard --matter <matter>`); point it at a real build or a stub;",
        "  `{{CLAUDE_TMP}}`: Claude's short temp folder; `{{ROOT}}`: anything else under the",
        "  test's temp root.",
        "",
        "A rule path `//{{MATTER}}/...` is Claude's absolute-path form: it stays right once",
        "`{{MATTER}}` becomes an absolute path. Rewrite before use, for example:",
        "",
        "```sh",
        "sed -e 's#{{PLUGINS}}#/abs/path/plugins#g' -e 's#{{DATA}}#/abs/data#g' \\",
        "    -e 's#{{MATTER}}#/abs/matter#g' -e 's#{{GUARD}}#/abs/clauseo#g' \\",
        "    -e 's#{{CLAUDE_TMP}}#/tmp/clsb-x#g' -e 's#{{ROOT}}#/abs/root#g' \\",
        "    emitted-mac.json > mac.json",
        "```",
        "",
        "## Built on @OS@",
        "",
        "These files were built on @OS@. The data folder's rules come from `deny_data_folder`",
        "(`crates/host/src/agent/loads/profile.rs`), which returns at once when the launch's",
        "`LaunchDeny` has no spellings, as this unit test's (`LaunchDeny::default()`) has none.",
        "So both files hold none of them: no data-folder `permissions.deny` or",
        "`permissions.allow` rules (`data_fence::claude_deny_rules`, `claude_allow_rules`), and",
        "no Windows `Bash`/`PowerShell` deny rules (`data_fence::windows_shell_deny_rules`),",
        "which are added only when built on Windows (`cfg!(windows)`) and only with spellings.",
        "The matter marker's `Edit` denies use the build OS's rule form, and the instructions in",
        "`systemPrompt` carry the macOS shell line only when built on macOS (both decided by the",
        "build's OS): the Mac file is exact when built on macOS, the Windows one on Windows.",
    ];
    lines
        .join("\n")
        .replace("@PLUGINS@", &serde_json::to_string_pretty(plugins).unwrap())
        .replace("@SOURCES@", &sources.to_string())
        .replace("@OS@", std::env::consts::OS)
        + "\n"
}

/// Writes the two settings files, both plugin folders and the README to `CLAUSEO_EMIT_DIR`;
/// does nothing when it is unset.
#[test]
#[ignore = "writes files outside the temp folder; run with CLAUSEO_EMIT_DIR set"]
fn writes_the_emitted_flag_settings() {
    let Some(out) = std::env::var_os(EMIT_VAR) else {
        return;
    };
    let out = PathBuf::from(out);
    let root = std::env::temp_dir().join(format!("clauseo-emit-{}", std::process::id()));
    crate::write::remove_dir_all(&root).unwrap();
    crate::write::create_dir_all(&root).unwrap();
    let rig = Rig::new(root.clone());
    let mac = json!({
        "forced_ask_off": rig.launch(true, false),
        "forced_ask_on": rig.launch(true, true),
    });
    let win = json!({
        "forced_ask_off": rig.launch(false, false),
        "forced_ask_on": rig.launch(false, true),
    });
    let left = format!("{mac}{win}");
    assert!(!left.contains(&text(&root)), "a path left unrewritten");
    crate::write::create_dir_all(&out).unwrap();
    for (name, value) in [("emitted-mac.json", &mac), ("emitted-win.json", &win)] {
        let body = serde_json::to_string_pretty(value).unwrap() + "\n";
        crate::write::write_file(&out.join(name), body.as_bytes()).unwrap();
    }
    for (from, name) in [
        (&rig.plugin, CLAUSEO_PLUGIN),
        (&rig.packs_plugin, PACKS_PLUGIN),
    ] {
        let to = out.join("plugins").join(name);
        crate::write::remove_dir_all(&to).unwrap();
        copy_tree(from, &to);
    }
    let first = &mac["forced_ask_off"];
    let notes = readme(&first["plugins"], &first["settingSources"]);
    crate::write::write_file(&out.join("README-emitted.md"), notes.as_bytes()).unwrap();
    crate::write::remove_dir_all(&root).unwrap();
}
