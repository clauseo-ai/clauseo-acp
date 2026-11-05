//! Brief S17's done-when 4: `limits`' path rules hold for an install laid out by `agents`.
//! The install folders come from `clauseo_agents::install_folder` with the embedded snapshot's
//! `<hash>`, the scripts from `clauseo_agents::script_path`, and the settings entries point
//! `node <script>` at them, as R20 will write them. Each read gets past every path rule to its
//! launch, which a refusing launcher then fails: no path is refused.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use clauseo_agents::{Platform, Snapshot, install_folder, script_path};
use clauseo_limits::{ProcessReaders, ReadFailure, ReadLimits, ReadOutcome};
use clauseo_process::{Error, LaunchSpec, Launched, Launcher};
use clauseo_protocol::{AgentId, AgentKind};
use futures::executor::block_on;

use crate::harness::{Setup, SharedTimer, under};
use crate::write;

/// What the refusing launcher answers.
const NOT_STARTED: &str = "the layout test starts nothing";

/// Records each spec and starts nothing.
#[derive(Default)]
struct Refusing {
    specs: Mutex<Vec<LaunchSpec>>,
}

impl Launcher for Refusing {
    fn launch(&self, spec: &LaunchSpec) -> Result<Launched, Error> {
        self.specs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(spec.clone());
        Err(Error::InvalidSpec(NOT_STARTED.to_owned()))
    }
}

/// The install folder and adapter script `agents` lays out for `id` under `data`.
fn laid_out(data: &Path, id: &str) -> (PathBuf, PathBuf) {
    let snapshot = Snapshot::embedded();
    let agent = AgentId::new(id).unwrap();
    let platform = Platform::current().expect("a platform Clauseo installs on");
    let hash = snapshot.install_hash(&agent, platform).unwrap();
    let folder = install_folder(data, &agent, &hash);
    let script = script_path(&folder, snapshot.agent(&agent).unwrap());
    (folder, script)
}

#[test]
fn limits_finds_both_adapters_and_the_claude_sdk_in_an_agents_install() {
    let setup = Setup::new();
    let data = setup.data();
    let (claude_folder, claude_script) = laid_out(&data, "claude-acp");
    let (codex_folder, codex_script) = laid_out(&data, "codex-acp");
    assert!(claude_folder.starts_with(data.join("agents").join("claude-acp").join("install")));
    assert_eq!(
        claude_script,
        under(
            &claude_folder,
            "node_modules/@agentclientprotocol/claude-agent-acp/dist/index.js"
        )
    );
    Setup::file(&claude_script, "");
    Setup::sdk(&claude_folder);
    Setup::file(&codex_script, "");
    let codex_js = under(&codex_folder, "node_modules/@openai/codex/bin/codex.js");
    Setup::file(&codex_js, "");
    write::create_dir_all(&setup.codex_home()).unwrap();

    let entry =
        |script: &Path| serde_json::json!({ "command": Setup::command(), "args": [script] });
    let settings = serde_json::json!({ "agents": {
        "claude-acp": entry(&claude_script),
        "codex-acp": entry(&codex_script),
    }});
    write::write(&data.join("settings.json"), settings.to_string()).unwrap();

    let launcher = Arc::new(Refusing::default());
    let readers = ProcessReaders::new(
        &setup.effective(),
        setup.codex_home(),
        Arc::<Refusing>::clone(&launcher) as Arc<dyn Launcher>,
        SharedTimer::default(),
    );
    for agent in [AgentKind::Claude, AgentKind::Codex] {
        let outcome = block_on(readers.read(agent));
        let ReadOutcome::Failed(ReadFailure::LaunchFailed(message)) = outcome else {
            panic!("{agent:?}: not the launcher's failure: {outcome:?}");
        };
        assert!(
            message.contains(NOT_STARTED),
            "{agent:?} was refused: {message}"
        );
    }
    let specs = launcher.specs.lock().unwrap();
    assert_eq!(specs.len(), 2, "both reads reached their launch");
    let helper = claude_folder.join("clauseo-claude-usage.mjs");
    assert_eq!(specs[0].args()[0], helper.as_os_str());
    assert!(
        helper.is_file(),
        "the helper is written into the agents install folder"
    );
    assert_eq!(specs[1].args()[0], codex_js.as_os_str());
}
