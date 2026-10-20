//! The fix lines of the tauri-config check, one variant per failure kind.
//! Each text is written once, in `Display`; `fix/tests.rs` pins every variant's text.

use std::fmt;

/// A fix line of the tauri-config check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Fix {
    /// `tauri.conf.json` is not there.
    ConfMissing,
    /// `tauri.conf.json` is not valid JSON.
    ConfInvalid,
    /// `app.withGlobalTauri` is not `false`.
    GlobalTauri,
    /// `app.security.freezePrototype` is not `true`.
    FreezePrototype,
    /// `app.security.csp` is not the pinned policy string.
    CspPinned,
    /// `build.frontendDist` is a URL or a list of files.
    Frontend,
    /// `app.windows` is not an empty array.
    Windows,
    /// `app.security.assetProtocol.enable` is `true`.
    AssetProtocol,
    /// A platform config exists and would override the checked settings.
    Platform,
    /// The updater has endpoints but no public key.
    Pubkey,
    /// `bundle.createUpdaterArtifacts` is `"v1Compatible"`.
    V1Compatible,
    /// A `dangerous*` key appears at some depth.
    Dangerous,
    /// `build.rs` declares no `AppManifest`.
    AppManifest,
    /// `build.rs` is not valid Rust.
    BuildRsSyntax,
}

impl fmt::Display for Fix {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Fix::ConfMissing => {
                "restore it: Tauri reads the window's security settings from it, and this check \
                 guards them"
            }
            Fix::ConfInvalid => "fix the JSON syntax: Tauri cannot read the file either",
            Fix::GlobalTauri => {
                "set `\"withGlobalTauri\": false` under `app`: the global object hands every \
                 script in the page the whole IPC API"
            }
            Fix::FreezePrototype => {
                "set `\"freezePrototype\": true` under app.security: a frozen prototype stops \
                 script in the page from hijacking the objects the IPC bridge uses"
            }
            Fix::CspPinned => {
                "set app.security.csp to the string in PINNED_CSP: the policy is pinned in \
                 xtask/src/checks/tauri_config.rs and changes only together with that const, in \
                 review"
            }
            Fix::Frontend => {
                "frontendDist must be the local dist folder; a URL makes the release app load a \
                 remote site"
            }
            Fix::Windows => {
                "build windows in crates/desktop/src/lib.rs: a config window skips \
                 use_https_scheme(true)"
            }
            Fix::AssetProtocol => {
                "set app.security.assetProtocol.enable to false: the asset protocol serves files \
                 from disk to the webview"
            }
            Fix::Platform => {
                "put the setting in tauri.conf.json: a platform config overrides the checked \
                 settings on one OS"
            }
            Fix::Pubkey => {
                "set plugins.updater.pubkey to the release public key before adding endpoints"
            }
            Fix::V1Compatible => {
                "the updater is built without its zip feature, so v1Compatible artifacts cannot be \
                 installed; use true"
            }
            Fix::Dangerous => "remove it: each `dangerous*` setting switches off a Tauri safeguard",
            Fix::AppManifest => {
                "pass `tauri_build::AppManifest::new()` to \
                 `tauri_build::Attributes::new().app_manifest(...)` in `tauri_build::try_build`: \
                 without a manifest Tauri skips the access check for app commands"
            }
            Fix::BuildRsSyntax => "fix the syntax: cargo cannot build the crate either",
        })
    }
}

#[cfg(test)]
mod tests;
