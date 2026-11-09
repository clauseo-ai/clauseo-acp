//! The browser socket folder and the network, the profile's last rules: [`Writer::write`]
//! writes them after every other file rule.

use super::{BROWSER_SOCKETS, Writer, ancestors, literal, subpath};
use crate::{Loopback, Policy};

/// The browser socket folder the host made: regular files and sockets in it may be made,
/// and files in it written, read and removed; the folder itself is neither made nor
/// removed, and no folder, link or pipe is made in it. Nothing else in `/tmp`.
pub(super) fn browser_sockets(w: &mut Writer, policy: &Policy) {
    let Some(dir) = policy.browser_socket_dir() else {
        return;
    };
    w.note("\n; The browser socket folder: agent-browser's socket and the files beside it.");
    let dir = w.param(BROWSER_SOCKETS, dir);
    w.rule(
        "allow",
        "file-read* file-write-data file-write-unlink",
        &[subpath(&dir)],
    );
    w.note("; Only regular files and sockets are made there: no folder, link or pipe.");
    // A swap (renamex_np, RENAME_SWAP) with a matter path can still bring a folder in;
    // Seatbelt cannot refuse it without refusing the matter's renames (0038).
    let made = ["REGULAR-FILE", "SOCKET"]
        .map(|kind| format!("(require-all {} (vnode-type {kind}))", subpath(&dir)));
    w.rule("allow", "file-write-create", &made);
    w.note("; The folder itself: not removed, not replaced (the host owns it).");
    w.rule(
        "deny",
        "file-write-create file-write-unlink",
        &[literal(&dir)],
    );
    w.note("; The folders above it: metadata only, so its path resolves.");
    w.rule("allow", "file-read-metadata", &[ancestors(&dir)]);
}

pub(super) fn network(w: &mut Writer, policy: &Policy) {
    w.note("\n; Network: the proxy's loopback ports and the browser socket folder only; no DNS.");
    let mut out: Vec<String> = policy
        .proxy_ports()
        .iter()
        .map(|port| format!("(remote ip \"localhost:{port}\")"))
        .collect();
    let sockets = policy
        .browser_socket_dir()
        .map(|dir| w.param(BROWSER_SOCKETS, dir));
    if let Some(dir) = &sockets {
        out.push(format!("(remote unix-socket (subpath {dir}))"));
    }
    let local: Vec<String> = match policy.loopback() {
        Loopback::None => Vec::new(),
        Loopback::Ports(ports) => ports
            .iter()
            .map(|port| format!("localhost:{port}"))
            .collect(),
        Loopback::Any => vec!["localhost:*".to_owned()],
    };
    out.extend(local.iter().map(|at| format!("(remote ip \"{at}\")")));
    w.rule("allow", "network-outbound", &out);
    if let Some(dir) = &sockets {
        w.note("; agent-browser's daemon, in the jail, makes its socket in its folder.");
        w.rule(
            "allow",
            "network-bind",
            &[format!("(local unix-socket (subpath {dir}))")],
        );
    }
    if !local.is_empty() {
        w.note("; The browser's loopback listeners.");
        let filters: Vec<String> = local
            .iter()
            .map(|at| format!("(local ip \"{at}\")"))
            .collect();
        w.rule("allow", "network-bind network-inbound", &filters);
    }
}
