//! The agent's bridge to the browser stack, which runs outside the jail under its own profile
//! (`macos::browser`, decision 0038): the guard's socket the agent connects to, the staging
//! folder it reads, the browser binaries it must never run, read, map or write, and the
//! browser's pins it must never write.

use std::path::{Path, PathBuf};

use super::build::{beneath_existing, existing, folder_and_name, refuse};
use super::{Loopback, Policy, PolicyBuilder, under};
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
    /// headless shells the app installs (`<data>/browsers`), and so the browser profile's
    /// `chrome_bundle` and `exec_bins`. The agent may not run, read, map or write anything under
    /// them; runtime paths may not lie under them, nor they under an agent-writable path.
    pub bundles: Vec<PathBuf>,
    /// Folders the agent may not write: the browser pins (`<data>/browser-pins`: the signed
    /// list, the host's install record, the floor). None may lie under an agent-writable path.
    pub write_denied: Vec<PathBuf>,
    /// The browser's own loopback ports, `DevTools` and stream: the agent's loopback and proxy
    /// ports may not include them, so the agent cannot drive Chrome around the guard.
    pub ports: Vec<u16>,
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
            let socket_dir = b.browser_socket_dir.as_deref();
            check(bridge, runtime, &b.loopback, &b.proxy_ports, socket_dir)
        })
        .transpose()
}

/// `bridge` canonical, checked against the rest of the policy: the staging folder must exist
/// and be `<…>/browser-runs/<run>/staging`, the socket must be that run's `sock/guard.sock`,
/// and with a bridge there may be no [`Loopback::Any`] (the browser's ports are the stack's,
/// outside the jail), no loopback or proxy port that is one of the browser's, no browser
/// socket folder (the bridge's literal socket replaces it), no runtime path equal to or inside
/// a bundle, and no bundle or write-denied folder meeting the runs folder.
pub(super) fn check(
    bridge: &BrowserBridge,
    runtime: &[PathBuf],
    loopback: &Loopback,
    proxy_ports: &[u16],
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
    let looped = match loopback {
        Loopback::Ports(ports) => ports.as_slice(),
        Loopback::None | Loopback::Any => &[],
    };
    if let Some(port) = looped
        .iter()
        .chain(proxy_ports)
        .find(|p| bridge.ports.contains(p))
    {
        return refuse(format!(
            "the agent's loopback and proxy ports may not include the browser's port {port}"
        ));
    }
    if let Some(dir) = socket_dir {
        return refuse(format!(
            "a browser socket folder may not be given with a browser bridge: {}",
            dir.display()
        ));
    }
    let canonical = |paths: &[PathBuf], what| {
        paths
            .iter()
            .map(|p| beneath_existing(p, what))
            .collect::<Result<Vec<_>, _>>()
    };
    let bundles = canonical(&bridge.bundles, "browser bundle")?;
    let write_denied = canonical(&bridge.write_denied, "browser write-denied folder")?;
    for bundle in bundles.iter().chain(&write_denied) {
        if under(bundle, runs) || under(runs, bundle) {
            return refuse(format!(
                "a browser bundle or write-denied folder may not meet the browser runs folder: {}",
                bundle.display()
            ));
        }
    }
    for bundle in &bundles {
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
        write_denied,
        ports: bridge.ports.clone(),
    })
}

/// `policy`, refused if a bundle or write-denied folder of its bridge meets a path the agent
/// may write (the matter, state, notes, temp and browser socket folders, the credential link,
/// the Claude store's names): planted code there would run outside the jail.
pub(super) fn verified(policy: Policy) -> Result<Policy, Error> {
    let Some(bridge) = policy.browser() else {
        return Ok(policy);
    };
    let mut writable: Vec<PathBuf> = vec![policy.matter().to_owned(), policy.state().to_owned()];
    writable.extend(policy.notes().map(Path::to_owned));
    writable.extend(policy.session_tmpdir().map(Path::to_owned));
    writable.extend(policy.credential_link().map(Path::to_owned));
    writable.extend(policy.browser_socket_dir().map(Path::to_owned));
    if let Some(store) = policy.claude_store() {
        writable.extend(store.files());
        writable.extend(store.lock_folders());
    }
    for protected in bridge.bundles.iter().chain(&bridge.write_denied) {
        if let Some(w) = writable
            .iter()
            .find(|w| under(protected, w) || under(w, protected))
        {
            return refuse(format!(
                "a browser bundle or write-denied folder may not meet a path the agent writes: \
                 {} and {}",
                protected.display(),
                w.display()
            ));
        }
    }
    Ok(policy)
}
