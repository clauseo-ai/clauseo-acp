//! The agent profile's rules for the browser stack, which runs outside the jail (0038, "The
//! browser profile"): with a [`BrowserBridge`](crate::BrowserBridge), a connect to the guard's
//! socket and a read of the run's staging folder; then, as last rules, no run, read or map of
//! the bridge's bundles, no write of them or its pins, nothing else in the run folders, and no
//! run or map of an installed browser, with a bridge or without.

use super::{Writer, ancestors, literal, subpath};
use crate::Policy;

/// The installed browsers, by bundle name: no `<name>.app` in `/Applications` or the home's
/// `Applications` may be run or mapped as code in the jail (it would run Chrome without its
/// sandbox, which cannot start in the jail). Reading stays: copying one into the matter and
/// running the copy is the residual decision 0038 names (T9). Seatbelt cannot match bundle ids.
pub const INSTALLED_BROWSERS: [&str; 15] = [
    "Google Chrome",
    "Google Chrome Beta",
    "Google Chrome Dev",
    "Google Chrome Canary",
    "Chromium",
    "Microsoft Edge",
    "Microsoft Edge Beta",
    "Microsoft Edge Dev",
    "Microsoft Edge Canary",
    "Brave Browser",
    "Brave Browser Beta",
    "Brave Browser Nightly",
    "Vivaldi",
    "Opera",
    "Arc",
];

/// `/(<name>|...)\.app/` in any letter case, for the folders an installed bundle can be in.
/// Seatbelt's regex has no case flag, so each letter is a class of both cases.
pub(super) fn installed_regex() -> String {
    let names: Vec<String> = INSTALLED_BROWSERS
        .iter()
        .map(|name| {
            name.chars()
                .map(|c| {
                    if c.is_ascii_alphabetic() {
                        format!("[{}{}]", c.to_ascii_lowercase(), c.to_ascii_uppercase())
                    } else {
                        c.to_string()
                    }
                })
                .collect()
        })
        .collect();
    format!("/({})\\.[aA][pP][pP](/|$)", names.join("|"))
}

/// The bridge's allows: after every other allow and before [`browser_denies`].
pub(super) fn browser_allows(w: &mut Writer, policy: &Policy) {
    let Some(bridge) = policy.browser() else {
        return;
    };
    w.note("\n; The browser stack: a connect to the guard's socket, the run's staging");
    w.note("; folder read-only. The stack itself runs outside the jail.");
    let socket = w.param("BROWSER_SOCKET", &bridge.socket);
    let staging = w.param("BROWSER_STAGING", &bridge.staging);
    w.rule(
        "allow",
        "network-outbound",
        &[format!("(remote unix-socket (path-literal {socket}))")],
    );
    w.rule("allow", "file-read*", &[subpath(&staging)]);
    w.rule(
        "allow",
        "file-read-metadata",
        &[ancestors(&staging), ancestors(&socket)],
    );
}

/// The browser's last rules, denies only, so no allow above can reopen them.
pub(super) fn browser_denies(w: &mut Writer, policy: &Policy) {
    w.note("\n; The browser's last rules: denies only.");
    if let Some(bridge) = policy.browser() {
        w.note("; The browser binaries: not run, read, mapped as code or written.");
        let bundles: Vec<String> = bridge
            .bundles
            .iter()
            .enumerate()
            .map(|(i, path)| subpath(&w.param(&format!("BROWSER_BUNDLE_{i}"), path)))
            .collect();
        w.rule(
            "deny",
            "process-exec file-read* file-map-executable file-write*",
            &bundles,
        );
        w.note("; The browser's pins (the signed list, the install record): not written.");
        let pins: Vec<String> = bridge
            .write_denied
            .iter()
            .enumerate()
            .map(|(i, path)| subpath(&w.param(&format!("BROWSER_WRITE_DENIED_{i}"), path)))
            .collect();
        w.rule("deny", "file-write*", &pins);
        let socket = w.param("BROWSER_SOCKET", &bridge.socket);
        let staging = w.param("BROWSER_STAGING", &bridge.staging);
        let runs = w.param("BROWSER_RUNS", bridge.runs());
        w.note("; The run folders: nothing written, run or mapped; nothing read but this");
        w.note("; run's staging folder, the folders above it and the guard's socket.");
        w.rule(
            "deny",
            "process-exec file-map-executable file-write*",
            &[subpath(&runs)],
        );
        let not_staging = format!("(require-not {})", subpath(&staging));
        w.rule(
            "deny",
            "file-read-data file-read-xattr",
            &[format!("(require-all {} {not_staging})", subpath(&runs))],
        );
        w.rule(
            "deny",
            "file-read-metadata",
            &[format!(
                "(require-all {} {not_staging} (require-not {}) (require-not {}))",
                subpath(&runs),
                ancestors(&staging),
                literal(&socket)
            )],
        );
    }
    w.note("; Installed browsers: not run or mapped as code (read stays; 0038, T9).");
    let regex = installed_regex();
    let mut installed = vec![format!("(regex #\"^/Applications{regex}\")")];
    if let Some(home) = policy.home() {
        let apps = w.param("HOME_APPLICATIONS", &home.join("Applications"));
        installed.push(format!(
            "(require-all {} (regex #\"{regex}\"))",
            subpath(&apps)
        ));
    }
    w.rule("deny", "process-exec file-map-executable", &installed);
}

#[cfg(test)]
mod tests;
