//! The profile's last rules: no keychain, its folders or the services that read it (0038).

use crate::Policy;

use super::{Writer, subpath};

/// The parameter naming the user's keychain folder, `<home>/Library/Keychains`.
const KEYCHAINS: &str = "KEYCHAINS";

/// The machine's keychain folder.
const SYSTEM_KEYCHAINS: &str = "/Library/Keychains";

/// The Mach services a keychain read goes through, each denied by name (0038).
const KEYCHAIN_SERVICES: [&str; 3] = [
    "com.apple.SecurityServer",
    "com.apple.securityd.xpc",
    "com.apple.securityd",
];

/// The prefixes of the Mach services behind the keychain, its tokens and its prompts, denied so
/// an allow added later cannot reach one by a name not listed above (0038).
const KEYCHAIN_SERVICE_PREFIXES: [&str; 6] = [
    "com.apple.security",
    "com.apple.secd",
    "com.apple.ctkd.",
    "com.apple.CoreAuthentication",
    "com.apple.authd",
    "com.apple.keychainsharingmessagingd",
];

/// No keychain, after every allow so none can reach it: a keychain item's access list
/// names programs, not jails, so an item that trusts a program the jail can run
/// (`/usr/bin/security`, `git-credential-osxkeychain`) would be readable from inside.
/// The default deny already refuses these; the explicit denies keep them refused (0038).
pub(super) fn write(w: &mut Writer, policy: &Policy) {
    w.note("\n; No keychain: not its folders, not the services that read it (0038).");
    let mut folders = Vec::new();
    if let Some(home) = policy.home() {
        let keychains = w.param(KEYCHAINS, &home.join("Library").join("Keychains"));
        folders.push(subpath(&keychains));
    }
    folders.push(format!("(subpath \"{SYSTEM_KEYCHAINS}\")"));
    w.rule("deny", "file-read* file-write*", &folders);
    let mut services: Vec<String> = KEYCHAIN_SERVICES
        .iter()
        .map(|name| format!("(global-name \"{name}\")"))
        .collect();
    services.extend(
        KEYCHAIN_SERVICE_PREFIXES
            .iter()
            .map(|prefix| format!("(global-name-prefix \"{prefix}\")")),
    );
    w.rule("deny", "mach-lookup", &services);
}
