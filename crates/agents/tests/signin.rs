//! S18's sign-in through the public API: the config's agent homes and base environment and the
//! public helpers here; the flows on the fake CLIs in `signin/<module>.rs`, each under
//! `#[cfg(test)]`, as `install.rs` does. The flows need `fake_agent` built first
//! (`cargo build -p clauseo-fake_agent`).

#[cfg(test)]
#[path = "signin/support.rs"]
mod support;

#[cfg(test)]
#[path = "signin/claude.rs"]
mod claude;

#[cfg(test)]
#[path = "signin/shim.rs"]
mod shim;

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::Path;

use clauseo_agents::{
    AgentsConfig, AgentsEvent, Platform, SigninConfig, SigninState, is_sign_in_again,
};
use clauseo_protocol::AgentId;
use clauseo_settings::Locations;

/// The agent homes R20 passes in are `Locations::agent_home`'s text: the layout the brief's
/// `CLAUDE_CONFIG_DIR` and `CODEX_HOME` name, `<data_dir>/agents/<id>/home`.
#[test]
fn agent_homes_come_from_settings_layout() {
    let root = Path::new("/data/Clauseo");
    let locations = Locations::under_root(root);
    for id in ["claude-acp", "codex-acp"] {
        assert_eq!(
            locations.agent_home(id),
            root.join("agents").join(id).join("home")
        );
    }
    let config = AgentsConfig::new(root.to_path_buf(), Platform::current().unwrap());
    assert!(config.signin.homes.is_empty(), "R20 sets the homes");
}

#[test]
fn the_config_never_prints_the_base_environment() {
    let mut signin = SigninConfig::from_env();
    signin.base_env = BTreeMap::from([(
        OsString::from("ANTHROPIC_API_KEY"),
        OsString::from("sk-not-for-logs"),
    )]);
    signin.homes.insert(
        AgentId::new("codex-acp").unwrap(),
        Path::new("/data/agents/codex-acp/home").to_path_buf(),
    );
    let printed = format!("{signin:?}");
    assert!(!printed.contains("sk-not-for-logs"), "{printed}");
    assert!(!printed.contains("ANTHROPIC_API_KEY"), "{printed}");
    assert!(
        printed.contains("1 names") && printed.contains("codex-acp"),
        "{printed}"
    );
}

#[test]
fn public_helpers_and_events() {
    assert!(is_sign_in_again("refresh_token_reused"));
    assert!(!is_sign_in_again("overloaded"));
    let event = AgentsEvent::Signin {
        agent: AgentId::new("claude-acp").unwrap(),
        state: SigninState::Queued,
    };
    assert!(matches!(
        event,
        AgentsEvent::Signin {
            state: SigninState::Queued,
            ..
        }
    ));
}
