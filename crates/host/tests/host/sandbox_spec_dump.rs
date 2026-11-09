//! A jailed launch's spec, dumped as JSON for the Windows VM's env-only row (#830): the host's
//! own spec builder makes it, through the launcher seam, so the VM launches `cmd /c set` and
//! `where git` with exactly the jailed environment. Ignored: it writes files, and it is run by
//! hand on the VM.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::Path;
use std::sync::Arc;

use clauseo_host::{NoProfile, ProfileHook, TestProxy};
use clauseo_process::LaunchSpec;
use clauseo_profile::PacksWritten;
use clauseo_protocol::{AgentId, StopReason, StreamId};
use serde_json::{Value, json};

use crate::profile_rig::CLAUDE;
use crate::sandbox_rig::{Answer, Fake, start_with};
use crate::support::{Dir, Scenario, matter_with_thread};

/// The test proxy's port: its browser and agent listener's.
const PROXY_PORT: u16 = 47_002;

/// The folder the dump goes to.
const DUMP_DIR_VAR: &str = "CLAUSEO_SPEC_DUMP_DIR";

/// The agent's settings `env`: two credential-shaped names and Claude Code's two shell names
/// the jail leaves out, one harmless name it passes.
fn settings_env() -> Value {
    json!({
        "TEST_API_KEY": "dummy-1",
        "PGPASSWORD": "dummy-2",
        "CLAUDE_CODE_GIT_BASH_PATH": "C:\\Users\\Public\\bash.exe",
        "CLAUDE_CODE_USE_POWERSHELL_TOOL": "0",
        "CLAUDE_CODE_MAX_OUTPUT_TOKENS": "1234",
    })
}

/// A profile hook giving Claude a dummy stored key, as desktop's gives the real one.
struct DummyKeyHook;

impl ProfileHook for DummyKeyHook {
    fn session_meta(
        &self,
        agent: &AgentId,
        packs: Option<&PacksWritten>,
    ) -> Option<serde_json::Map<String, Value>> {
        NoProfile.session_meta(agent, packs)
    }

    fn launch_env(&self, _agent: &AgentId) -> BTreeMap<OsString, OsString> {
        BTreeMap::from([("ANTHROPIC_API_KEY".into(), "dummy-0".into())])
    }
}

/// Rewrites `settings.json` with `env` on `agent`.
fn set_agent_env(dir: &Dir, agent: &str, env: Value) {
    let mut settings: Value = serde_json::from_str(&dir.settings_json()).unwrap();
    settings["agents"][agent]["env"] = env;
    let text = serde_json::to_string_pretty(&settings).unwrap();
    crate::write::write(&dir.data().join("settings.json"), text.as_bytes());
}

fn text(value: &std::ffi::OsStr) -> String {
    value
        .to_str()
        .unwrap_or_else(|| panic!("not UTF-8: {value:?}"))
        .to_owned()
}

fn path_text(path: &Path) -> String {
    text(path.as_os_str())
}

/// `spec` as the s15 harness's `exec-spec --spec-json` reads it.
fn spec_json(agent: &str, spec: &LaunchSpec) -> Value {
    let env: serde_json::Map<String, Value> = spec
        .env()
        .iter()
        .map(|(name, value)| (text(name), Value::String(text(value))))
        .collect();
    json!({
        "agent": agent,
        "jailed": true,
        "program": path_text(spec.program()),
        "args": spec.args().iter().map(|arg| text(arg)).collect::<Vec<_>>(),
        "cwd": path_text(spec.cwd()),
        "env": env,
    })
}

/// The checks the dump doubles as, on any OS.
fn assert_jailed_env(spec: &LaunchSpec) {
    let has = |name: &str| spec.env().contains_key(&OsString::from(name));
    for name in [
        "TEST_API_KEY",
        "PGPASSWORD",
        "CLAUDE_CODE_GIT_BASH_PATH",
        "CLAUDE_CODE_USE_POWERSHELL_TOOL",
    ] {
        assert!(!has(name), "{name} reached the jailed spec");
    }
    assert_eq!(
        spec.env()
            .get(&OsString::from("CLAUDE_CODE_MAX_OUTPUT_TOKENS"))
            .and_then(|value| value.to_str()),
        Some("1234")
    );
    #[cfg(windows)]
    {
        for name in [
            "ComSpec",
            "SystemDrive",
            "ProgramData",
            "ProgramFiles",
            "ProgramFiles(x86)",
            "ProgramW6432",
            "CommonProgramFiles",
            "CommonProgramFiles(x86)",
            "CommonProgramW6432",
            "PROCESSOR_ARCHITECTURE",
            "NUMBER_OF_PROCESSORS",
            "OS",
        ] {
            assert!(has(name), "{name} missing from the jailed spec");
        }
        // Claude Code's PowerShell tool, its shell without Git Bash, is on the jailed PATH.
        let root = spec
            .env()
            .get(&OsString::from("SystemRoot"))
            .expect("SystemRoot");
        let powershell = Path::new(root)
            .join("System32")
            .join("WindowsPowerShell")
            .join("v1.0");
        let path = spec
            .env()
            .get(&OsString::from("PATH"))
            .and_then(|path| path.to_str())
            .expect("PATH");
        // Windows' separator, split as text: the folders hold no `;`.
        assert!(
            path.split(';')
                .any(|folder| Path::new(folder) == powershell),
            "{path}"
        );
        // The profile names are host-set (`jailed_env::JAILED_ENV`) but the host gives them no
        // value: the Windows runner sets them at the jailed home whatever the spec says
        // (`clauseo_sandbox::windows::runner::env`). So the spec carries none, and no inherited,
        // hooked or settings value reaches it; the VM row checks the runner's values.
        for name in [
            "USERPROFILE",
            "APPDATA",
            "LOCALAPPDATA",
            "HOMEDRIVE",
            "HOMEPATH",
        ] {
            assert!(!has(name), "{name} in the jailed spec: the runner sets it");
        }
    }
}

/// Dumps a jailed Claude's launch spec, built by the host through the launcher seam, to
/// `$CLAUSEO_SPEC_DUMP_DIR/claude.json` for the s15 harness's `exec-spec --spec-json`
/// (skipped when the variable is unset). Its settings `env` holds `TEST_API_KEY` and
/// `PGPASSWORD`, `CLAUDE_CODE_GIT_BASH_PATH` and `CLAUDE_CODE_USE_POWERSHELL_TOOL` (left out)
/// and `CLAUDE_CODE_MAX_OUTPUT_TOKENS` (kept), which it asserts; on Windows its `PATH` holds
/// `<SystemRoot>\System32\WindowsPowerShell\v1.0`.
///
/// A jailed Codex is refused before its spec is built until R17 PR-D2c
/// (`sandbox_policy::a_jailed_codex_is_refused_before_any_reservation_or_launcher`), so there is
/// no Codex dump yet.
///
/// The program is the rig's fake agent and the paths (cwd, state folders, CA bundle) are the
/// test's own temp folders, removed when it ends: the env's names and rules are what the VM row
/// uses, not those paths.
///
/// On the VM (cmd; the rig needs the fake agent built first):
///
/// ```text
/// cargo build -p clauseo-fake_agent && set CLAUSEO_SPEC_DUMP_DIR=C:\vm\specs&& cargo test -p clauseo-host --test host sandbox_spec_dump::dump_jailed_launch_specs_for_the_vm -- --ignored --exact
/// ```
#[test]
#[ignore = "writes the jailed spec for the Windows VM: set CLAUSEO_SPEC_DUMP_DIR, run with --ignored"]
fn dump_jailed_launch_specs_for_the_vm() {
    let Some(out) = std::env::var_os(DUMP_DIR_VAR) else {
        eprintln!("{DUMP_DIR_VAR} is unset: nothing dumped");
        return;
    };
    let dir = Dir::new(
        "sandbox_spec_dump_claude",
        &[(CLAUDE, Scenario::Bundled("claude-prompt-stream"))],
    );
    set_agent_env(&dir, CLAUDE, settings_env());
    let (fake, _) = Fake::new(&dir, Answer::Launch(Vec::new()));
    let (host, _, _) = start_with(&dir, Some(Arc::clone(&fake)), |config| {
        config
            .with_egress_proxy(Arc::new(TestProxy::new(PROXY_PORT)))
            .with_profile_hook(Box::new(DummyKeyHook))
    });
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("a.txt", "x\n")], CLAUDE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Look at the matter.");
    assert_eq!(
        events.until_turn_ended(turn).stop,
        StopReason::EndTurn,
        "{}",
        events.dump()
    );
    let specs = fake.specs();
    let spec = specs.first().expect("one jailed launch");
    assert_jailed_env(spec);

    crate::write::create_dir_all(Path::new(&out));
    let file = Path::new(&out).join("claude.json");
    let body = serde_json::to_string_pretty(&spec_json(CLAUDE, spec)).unwrap();
    crate::write::write(&file, body.as_bytes());
    eprintln!("wrote {}", file.display());
}
