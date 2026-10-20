//! Pins the text of every `tauri_config::Fix` variant. The `match` has no `_` arm, so a new variant
//! does not compile until it has a pinned text here.

use super::Fix;
use std::collections::HashSet;
use std::mem::discriminant;

fn pinned(fix: &Fix) -> &'static str {
    match fix {
        Fix::ConfMissing => {
            "restore it: Tauri reads the window's security settings from it, and this check guards \
             them"
        }
        Fix::ConfInvalid => "fix the JSON syntax: Tauri cannot read the file either",
        Fix::GlobalTauri => {
            "set `\"withGlobalTauri\": false` under `app`: the global object hands every script in \
             the page the whole IPC API"
        }
        Fix::FreezePrototype => {
            "set `\"freezePrototype\": true` under app.security: a frozen prototype stops script in \
             the page from hijacking the objects the IPC bridge uses"
        }
        Fix::CspPinned => {
            "set app.security.csp to the string in PINNED_CSP: the policy is pinned in \
             xtask/src/checks/tauri_config.rs and changes only together with that const, in review"
        }
        Fix::Frontend => {
            "frontendDist must be the local dist folder; a URL makes the release app load a remote \
             site"
        }
        Fix::Windows => {
            "build windows in crates/desktop/src/lib.rs: a config window skips use_https_scheme(true)"
        }
        Fix::AssetProtocol => {
            "set app.security.assetProtocol.enable to false: the asset protocol serves files from \
             disk to the webview"
        }
        Fix::Platform => {
            "put the setting in tauri.conf.json: a platform config overrides the checked settings on \
             one OS"
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
            "pass `tauri_build::AppManifest::new()` to `tauri_build::Attributes::new().app_manifest(...)` \
             in `tauri_build::try_build`: without a manifest Tauri skips the access check for app \
             commands"
        }
        Fix::BuildRsSyntax => "fix the syntax: cargo cannot build the crate either",
        Fix::ConfigInCode => {
            "remove it and put the setting in tauri.conf.json: Tauri reads or merges a config \
             passed in code past this check"
        }
        Fix::ContextArguments => {
            "call `tauri::generate_context!()` with no arguments: it then reads the checked \
             tauri.conf.json and capabilities folder"
        }
        Fix::ContextRenamed => {
            "import it under its own name and call `generate_context!()`, which this check reads"
        }
        Fix::SourceSyntax => "fix the syntax: this check cannot read the file's calls",
        Fix::ConfigInEnv => {
            "remove it and put the setting in tauri.conf.json: Tauri merges `TAURI_CONFIG` over \
             the checked file"
        }
    }
}

#[test]
fn every_fix_prints_its_pinned_text() {
    let samples = [
        Fix::ConfMissing,
        Fix::ConfInvalid,
        Fix::GlobalTauri,
        Fix::FreezePrototype,
        Fix::CspPinned,
        Fix::Frontend,
        Fix::Windows,
        Fix::AssetProtocol,
        Fix::Platform,
        Fix::Pubkey,
        Fix::V1Compatible,
        Fix::Dangerous,
        Fix::AppManifest,
        Fix::BuildRsSyntax,
        Fix::ConfigInCode,
        Fix::ContextArguments,
        Fix::ContextRenamed,
        Fix::SourceSyntax,
        Fix::ConfigInEnv,
    ];
    let kinds: HashSet<_> = samples.iter().map(discriminant).collect();
    assert_eq!(
        kinds.len(),
        samples.len(),
        "two samples are the same variant"
    );
    for fix in &samples {
        assert_eq!(fix.to_string(), pinned(fix));
    }
}
