//! The downloaded Node's checks and an agent's finished install, read at each start and at
//! each job's start.

use clauseo_process::{Launcher, Timer};
use clauseo_protocol::AgentId;

use crate::config::AgentsConfig;
use crate::node::{self, NotUsed, UserSearch};
use crate::receipt::{install_folder, read_receipt, script_path, write_receipt};
use crate::state::Installed;

/// Whether the downloaded Node is unpacked and complete.
pub(crate) fn runtime_ready(config: &AgentsConfig) -> bool {
    let Some(archive) = config.snapshot.node_archive(config.platform) else {
        return false;
    };
    let dir = node::runtime_dir(&config.data_dir, config.snapshot.node(), config.platform);
    node::runtime_valid(&dir, archive, config.platform.os)
}

/// What the start's check of the downloaded Node found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeCheck {
    /// Not unpacked, or its receipt is missing or for another archive.
    Absent,
    /// It answers `--version` with v22 or later.
    Good,
    /// `--version` took too long or would not start: not proof of a bad Node, so it is kept and
    /// checked again at the next install or start.
    Unsure,
    /// A clear bad answer (a non-zero exit, no version, or one below 22): fetched again.
    Bad,
}

impl RuntimeCheck {
    /// Whether the runtime is used as it is.
    pub(crate) fn usable(self) -> bool {
        matches!(self, RuntimeCheck::Good | RuntimeCheck::Unsure)
    }
}

/// Checks the downloaded Node: unpacked and complete, then `--version` through the launcher, so
/// a runtime cut short by a power cut is fetched again.
pub(crate) async fn runtime_check<T: Timer>(
    config: &AgentsConfig,
    launcher: &dyn Launcher,
    timer: &T,
) -> RuntimeCheck {
    if !runtime_ready(config) {
        return RuntimeCheck::Absent;
    }
    let dir = node::runtime_dir(&config.data_dir, config.snapshot.node(), config.platform);
    let program = node::runtime_program(&dir, config.platform.os);
    match node::check_node(&program, launcher, timer).await {
        Ok(_) => RuntimeCheck::Good,
        Err(NotUsed::TimedOut | NotUsed::LaunchFailed(_)) => RuntimeCheck::Unsure,
        Err(_) => RuntimeCheck::Bad,
    }
}

/// `agent`'s finished install: a valid receipt in its `<hash>` folder, run with a Node this
/// start would choose anyway. The receipt's Node is kept when it is the downloaded runtime and
/// usable ([`RuntimeCheck::usable`]), or the Node the `PATH` search finds now, or one that
/// search could not finish checking. Otherwise the receipt is rewritten (atomically) to name the
/// Node found now, else the usable runtime, and stays ready: the adapter runs on any v22 or
/// later. With neither, `None`, and the install is redone. A receipt never makes a program run
/// that this start would not choose (the data folder is trusted at rest, but a receipt marks
/// completeness, not which program may run).
pub(crate) async fn ready_install<T: Timer>(
    config: &AgentsConfig,
    launcher: &dyn Launcher,
    timer: &T,
    agent: &AgentId,
    user: &UserSearch,
) -> Option<Installed> {
    let entry = config.snapshot.agent(agent)?;
    let hash = config.snapshot.install_hash(agent, config.platform)?;
    let folder = install_folder(&config.data_dir, agent, &hash);
    let mut receipt = read_receipt(&folder, agent, &hash, config.platform)?;
    let runtime = node::runtime_dir(&config.data_dir, config.snapshot.node(), config.platform);
    let runtime = node::runtime_program(&runtime, config.platform.os);
    let mut runtime_usable = None;
    let keep = if receipt.node == runtime {
        let usable = runtime_check(config, launcher, timer).await.usable();
        runtime_usable = Some(usable);
        usable
    } else {
        user.found
            .as_ref()
            .is_some_and(|found| found.program == receipt.node)
            || user.timed_out.contains(&receipt.node)
    };
    if !keep {
        let (program, version) = if let Some(found) = &user.found {
            (found.program.clone(), found.version.clone())
        } else {
            let usable = match runtime_usable {
                Some(usable) => usable,
                None => runtime_check(config, launcher, timer).await.usable(),
            };
            if !usable {
                return None;
            }
            (
                runtime.clone(),
                format!("v{}", config.snapshot.node().version()),
            )
        };
        receipt.node = program;
        receipt.node_version = version;
        // A receipt that will not be rewritten now is rewritten at the next start.
        drop(write_receipt(&folder, &receipt));
    }
    tracing::info!(%agent, "agent start: {}", receipt.node_line(&runtime));
    Some(Installed {
        node: receipt.node.clone(),
        script: script_path(&folder, entry),
        folder,
        version: receipt.version,
    })
}
