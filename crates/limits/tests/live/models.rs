//! One live model probe (`ProcessReaders::probe_models`): no prompt, no conversation. It prints
//! the model names, each with its effort names, and the defaults; no id that could be a token.

use std::path::PathBuf;
use std::sync::Arc;

use clauseo_limits::{ProcessReaders, ReadLimits};
use clauseo_process::{AsyncIoTimer, Launcher, PassthroughLauncher, SpawnOptions};
use clauseo_protocol::AgentKind;
use clauseo_settings::{Effective, Locations, SystemFiles};
use futures::executor::block_on;

use crate::run::{INSTALL, Kind, NODE, Scratch, required, settings};
use crate::write;

/// Probes `kind`'s models once, or prints its skip line when its login variable is unset.
pub(crate) fn live_models(kind: Kind) {
    let var = kind.var();
    let Some(login) = std::env::var_os(&var).filter(|value| !value.is_empty()) else {
        println!("skipped: no logins ({var})");
        return;
    };
    let (install, node) = (required(INSTALL), required(NODE));
    let scratch = Scratch::new(&format!("{}-models", kind.name()));
    let data = scratch.0.join("data");
    write::create_dir_all(&data).unwrap();
    let settings = settings(kind, &install, &node, &login);
    write::write(&data.join("settings.json"), settings.to_string()).unwrap();
    let effective = Effective::load(&Locations::under_root(&data), &SystemFiles, &[]);
    let codex_home = match kind.agent() {
        AgentKind::Codex => PathBuf::from(&login),
        AgentKind::Claude => scratch.0.join("codex-home"),
    };
    let launcher: Arc<dyn Launcher> = Arc::new(PassthroughLauncher::new(SpawnOptions::new()));
    let readers = ProcessReaders::new(&effective, codex_home, launcher, AsyncIoTimer);
    let models = block_on(readers.probe_models(kind.agent())).expect("the probe answered");
    println!(
        "{} models (current {} at {:?}):",
        kind.name(),
        models.current_model,
        models.current_effort
    );
    for model in &models.models {
        let efforts: Vec<&str> = model
            .efforts
            .iter()
            .map(|effort| effort.name.as_str())
            .collect();
        println!(
            "  {} [{}] efforts: {} (default {:?})",
            model.name,
            model.id,
            efforts.join(", "),
            model.default_effort
        );
    }
    assert_ne!(models.models, []);
}
