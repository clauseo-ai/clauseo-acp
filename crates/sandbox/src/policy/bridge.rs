//! The agent's bridge to the browser stack, which runs outside the jail under its own profile
//! (`macos::browser`, decision 0038): the guard's socket the agent connects to, the staging
//! folder it reads, and the browser binaries it must never run, read or map.

use std::path::{Path, PathBuf};

use super::build::{beneath_existing, existing, folder_and_name, refuse};
use super::{Loopback, PolicyBuilder, under};
use crate::Error;
use crate::macos::browser::GUARD_SOCKET;

/// What the agent's jail opens to the browser stack, and what it closes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserBridge {
    /// The guard's Unix socket, `<data>/browser-runs/<run>/sock/guard.sock`: the agent may
    /// connect to this one path (a literal rule); it may not bind, make or remove anything in
    /// `sock/` or anywhere else in the run folders.
    pub socket: PathBuf,
    /// The run's staging folder, `<data>/browser-runs/<run>/staging`: read-only to the agent.
    pub staging: PathBuf,
    /// The browser binaries: agent-browser's folder in the app (`Contents/Helpers`), the
    /// browsers the app downloads (`<data>/browsers`), any Chrome the host knows of. The agent
    /// may not run, read or map anything under them; runtime paths may not lie under them.
    pub bundles: Vec<PathBuf>,
}

impl BrowserBridge {
    /// The folder holding every run folder, `<data>/browser-runs`: the staging folder's
    /// grandparent, which [`check`] made sure of.
    pub fn runs(&self) -> &Path {
        self.staging
            .parent()
            .and_then(Path::parent)
            .unwrap_or(&self.staging)
    }
}

/// The builder's bridge, if any, by [`check`].
pub(super) fn checked(
    b: &PolicyBuilder,
    runtime: &[PathBuf],
) -> Result<Option<BrowserBridge>, Error> {
    b.browser
        .as_ref()
        .map(|bridge| {
            check(
                bridge,
                runtime,
                &b.loopback,
                b.browser_socket_dir.as_deref(),
            )
        })
        .transpose()
}

/// `bridge` canonical, checked against the rest of the policy: the staging folder must exist
/// and be `<…>/browser-runs/<run>/staging`, the socket must be that run's `sock/guard.sock`,
/// and with a bridge there may be no [`Loopback::Any`] (the browser's ports are the stack's,
/// outside the jail), no browser socket folder (the bridge's literal socket replaces it), no
/// runtime path equal to or inside a bundle, and no bundle meeting the runs folder.
pub(super) fn check(
    bridge: &BrowserBridge,
    runtime: &[PathBuf],
    loopback: &Loopback,
    socket_dir: Option<&Path>,
) -> Result<BrowserBridge, Error> {
    let staging = existing(&bridge.staging, "browser staging folder")?;
    let run = staging.parent().filter(|_| staging.ends_with("staging"));
    let runs = run.and_then(Path::parent);
    let Some((run, runs)) = run.zip(runs).filter(|(_, r)| r.ends_with("browser-runs")) else {
        return refuse(format!(
            "the browser staging folder must be <…>/browser-runs/<run>/staging: {}",
            staging.display()
        ));
    };
    let socket = folder_and_name(&bridge.socket, "browser socket")?;
    let want = run.join("sock").join(GUARD_SOCKET);
    if socket != want {
        return refuse(format!(
            "the browser socket must be {}: {}",
            want.display(),
            socket.display()
        ));
    }
    if matches!(loopback, Loopback::Any) {
        return refuse("the browser's loopback may not be any port with a browser bridge".into());
    }
    if let Some(dir) = socket_dir {
        return refuse(format!(
            "a browser socket folder may not be given with a browser bridge: {}",
            dir.display()
        ));
    }
    let bundles = bridge
        .bundles
        .iter()
        .map(|b| beneath_existing(b, "browser bundle"))
        .collect::<Result<Vec<_>, _>>()?;
    for bundle in &bundles {
        if under(bundle, runs) || under(runs, bundle) {
            return refuse(format!(
                "a browser bundle may not meet the browser runs folder: {}",
                bundle.display()
            ));
        }
        if let Some(path) = runtime.iter().find(|p| under(p, bundle)) {
            return refuse(format!(
                "a runtime path may not be inside a browser bundle: {} in {}",
                path.display(),
                bundle.display()
            ));
        }
    }
    Ok(BrowserBridge {
        socket,
        staging,
        bundles,
    })
}
