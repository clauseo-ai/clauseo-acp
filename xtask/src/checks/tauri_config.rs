//! The `tauri-config` check: the security settings of the desktop crate's `tauri.conf.json` and
//! `build.rs`.
//!
//! In `tauri.conf.json`: the global Tauri object is off, the prototype is frozen, the CSP is
//! exactly `PINNED_CSP`, `build.frontendDist` is a local folder (no URL, no file list), no window
//! is built from config, the asset protocol is off, and no `dangerous*` key appears at any depth.
//! The updater has a public key once it has endpoints, and `bundle.createUpdaterArtifacts` is not
//! `"v1Compatible"`, which needs the updater's `zip` feature that the workspace leaves off.
//! No platform config (`tauri.<platform>.conf.json`, `.json5` or `Tauri.<platform>.toml`) exists:
//! Tauri merges one over `tauri.conf.json` and could turn a setting back on for one OS.
//! In `build.rs`: an `AppManifest` is declared (`app_manifest(` and `AppManifest::new()`, outside
//! comments), and `capabilities_path_pattern(` does not point Tauri at another folder. Each failing
//! setting points at its key's line, found under its parent object (`json_text::key_line`), or at
//! line 1.
//! The pure functions take file contents and paths; `run` reads the files from the repo and counts
//! the ones it found: `tauri.conf.json`, `build.rs` and each platform config.

use crate::checks::json_text::key_line;
use crate::checks::{self, Inspection, Noun, StaticCheck, violation};
use crate::violation::Violation;
use crate::workspace::Context;
use serde_json::Value;

/// The files this check reads, inside the desktop crate's directory.
const TAURI_CONF: &str = "tauri.conf.json";
const BUILD_RS: &str = "build.rs";

const CONF_MISSING_FIX: &str =
    "restore it: Tauri reads the window's security settings from it, and this check guards them";
const CONF_INVALID_FIX: &str = "fix the JSON syntax: Tauri cannot read the file either";
const GLOBAL_TAURI_FIX: &str = "set `\"withGlobalTauri\": false` under `app`: the global object hands every script in the page the whole IPC API";
const FREEZE_FIX: &str = "set `\"freezePrototype\": true` under app.security: a frozen prototype stops script in the page from hijacking the objects the IPC bridge uses";
const CSP_PINNED_FIX: &str = "set app.security.csp to the string in PINNED_CSP: the policy is pinned in xtask/src/checks/tauri_config.rs and changes only together with that const, in review";
const FRONTEND_FIX: &str =
    "frontendDist must be the local dist folder; a URL makes the release app load a remote site";
const WINDOWS_FIX: &str =
    "build windows in crates/desktop/src/lib.rs: a config window skips use_https_scheme(true)";
const ASSET_FIX: &str = "set app.security.assetProtocol.enable to false: the asset protocol serves files from disk to the webview";
const PLATFORM_FIX: &str = "put the setting in tauri.conf.json: a platform config overrides the checked settings on one OS";
const PUBKEY_FIX: &str =
    "set plugins.updater.pubkey to the release public key before adding endpoints";
const V1_FIX: &str = "the updater is built without its zip feature, so v1Compatible artifacts cannot be installed; use true";
const DANGEROUS_FIX: &str = "remove it: each `dangerous*` setting switches off a Tauri safeguard";
const APP_MANIFEST_FIX: &str = "pass `tauri_build::AppManifest::new()` to `tauri_build::Attributes::new().app_manifest(...)` in `tauri_build::try_build`: without a manifest Tauri skips the access check for app commands";
const PATH_PATTERN_FIX: &str = "remove it: capabilities live in the crate's capabilities folder, the one the capabilities check scans";

/// The platforms Tauri merges a platform config for, over `tauri.conf.json`.
const PLATFORMS: [&str; 5] = ["macos", "windows", "linux", "ios", "android"];

/// The one CSP `app.security.csp` may hold. The policy is decided, so a change to it is an edit
/// here too, which puts it in front of a reviewer.
pub(crate) const PINNED_CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data: clauseo-file: http://clauseo-file.localhost https://clauseo-file.localhost; font-src 'self'; connect-src ipc: http://ipc.localhost https://ipc.localhost; worker-src 'self'; frame-src 'self'; child-src 'self'; object-src 'none'; media-src 'none'; manifest-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'; require-trusted-types-for 'script'; trusted-types clauseo-sanitize";

/// Fails on every unsafe setting in `tauri.conf.json` and on a `build.rs` without an `AppManifest`.
pub(crate) const CHECK: StaticCheck = StaticCheck {
    name: "tauri-config",
    counts: Noun {
        one: "config file",
        many: "config files",
    },
    place: "in the desktop crate (tauri.conf.json and build.rs)",
    source: "xtask/src/checks/tauri_config.rs",
    run,
};

fn run(ctx: &Context) -> Inspection {
    let desktop = match checks::desktop_dir(ctx) {
        Ok(dir) => dir,
        Err(violation) => return Inspection::stopped(violation),
    };
    let mut read = 0;
    let conf = checks::join(desktop, TAURI_CONF);
    let mut found = match checks::read_repo_file(&ctx.root, &conf) {
        Ok(text) => {
            read += usize::from(text.is_some());
            conf_violations(&conf, text.as_deref())
        }
        Err(violation) => vec![violation],
    };
    let platform: Vec<String> = platform_configs()
        .into_iter()
        .map(|name| checks::join(desktop, &name))
        .filter(|rel| ctx.root.join(rel).exists())
        .collect();
    read += platform.len();
    found.extend(platform.iter().map(|rel| platform_violation(rel)));
    let build = checks::join(desktop, BUILD_RS);
    found.extend(match checks::read_repo_file(&ctx.root, &build) {
        Ok(text) => {
            read += usize::from(text.is_some());
            build_rs_violations(&build, text.as_deref())
        }
        Err(violation) => vec![violation],
    });
    Inspection::new(read, found)
}

/// Every unsafe setting in the Tauri config at `path`. `None` means the file is missing; a missing
/// file or invalid JSON is one violation at line 1.
pub(crate) fn conf_violations(path: &str, text: Option<&str>) -> Vec<Violation> {
    let Some(text) = text else {
        return vec![violation(
            path,
            1,
            format!("`{path}` is missing"),
            CONF_MISSING_FIX,
        )];
    };
    let conf: Value = match serde_json::from_str(text) {
        Ok(conf) => conf,
        Err(error) => {
            return vec![violation(
                path,
                1,
                format!("`{path}` is not valid JSON: {error}"),
                CONF_INVALID_FIX,
            )];
        }
    };
    let at = |keys: &[&str], message: String, fix: &str| {
        violation(path, key_line(text, keys), message, fix)
    };
    let mut found = app_violations(&conf, &at);
    found.extend(csp_violations(&conf, &at));
    found.extend(frontend_violations(&conf, &at));
    found.extend(updater_violations(&conf, &at));
    for keys in dangerous_keys(&conf) {
        let keys: Vec<&str> = keys.iter().map(String::as_str).collect();
        found.push(at(
            &keys,
            format!("`{}` switches off a Tauri safeguard", keys.join(".")),
            DANGEROUS_FIX,
        ));
    }
    found
}

/// Builds the violation for a message and fix at the line of the key on a path.
type At<'a> = dyn Fn(&[&str], String, &str) -> Violation + 'a;

/// The global object, the frozen prototype, config windows and the asset protocol.
fn app_violations(conf: &Value, at: &At<'_>) -> Vec<Violation> {
    let mut found = Vec::new();
    if conf.pointer("/app/withGlobalTauri") != Some(&Value::Bool(false)) {
        found.push(at(
            &["app", "withGlobalTauri"],
            "app.withGlobalTauri must be false".to_owned(),
            GLOBAL_TAURI_FIX,
        ));
    }
    if conf.pointer("/app/security/freezePrototype") != Some(&Value::Bool(true)) {
        found.push(at(
            &["app", "security", "freezePrototype"],
            "app.security.freezePrototype must be true".to_owned(),
            FREEZE_FIX,
        ));
    }
    if !conf
        .pointer("/app/windows")
        .and_then(Value::as_array)
        .is_some_and(Vec::is_empty)
    {
        found.push(at(
            &["app", "windows"],
            "app.windows must be an empty array".to_owned(),
            WINDOWS_FIX,
        ));
    }
    if conf.pointer("/app/security/assetProtocol/enable") == Some(&Value::Bool(true)) {
        found.push(at(
            &["app", "security", "assetProtocol", "enable"],
            "app.security.assetProtocol.enable is true".to_owned(),
            ASSET_FIX,
        ));
    }
    found
}

/// The CSP is exactly `PINNED_CSP`: a string, not an object of directives.
fn csp_violations(conf: &Value, at: &At<'_>) -> Vec<Violation> {
    let message = match conf.pointer("/app/security/csp") {
        Some(Value::String(csp)) if csp == PINNED_CSP => return Vec::new(),
        Some(Value::String(_)) => "app.security.csp is not the pinned policy",
        Some(Value::Object(_)) => "app.security.csp is an object, not the pinned policy string",
        Some(_) => "app.security.csp is not a string",
        None => "app.security.csp is missing",
    };
    vec![at(
        &["app", "security", "csp"],
        message.to_owned(),
        CSP_PINNED_FIX,
    )]
}

/// `build.frontendDist` names the local dist folder: not a URL, which Tauri would load as a remote
/// site, and not an array, which Tauri reads as a list of files to embed.
fn frontend_violations(conf: &Value, at: &At<'_>) -> Vec<Violation> {
    let message = match conf.pointer("/build/frontendDist") {
        Some(Value::String(dist)) if is_url(dist) => "build.frontendDist is a URL",
        Some(Value::Array(_)) => "build.frontendDist is a list of files, not the dist folder",
        _ => return Vec::new(),
    };
    vec![at(
        &["build", "frontendDist"],
        message.to_owned(),
        FRONTEND_FIX,
    )]
}

/// True when `text` starts with a URL scheme, `^[A-Za-z][A-Za-z0-9+.-]*:`, the syntax
/// `url::Url::parse` accepts. Tauri tries `Url` first for `frontendDist`, so anything with a
/// scheme (`file:`, `ftp:`, `data:` as well as `http:`) is a URL to it. A Windows drive path
/// (`C:\\dist`) matches too, and Tauri would misread it as a URL anyway.
fn is_url(text: &str) -> bool {
    let Some((scheme, _)) = text.trim().split_once(':') else {
        return false;
    };
    let mut chars = scheme.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'))
}

/// Endpoints need the public key that verifies updates, and no artifacts in the v1 format, which
/// needs the updater's `zip` feature.
fn updater_violations(conf: &Value, at: &At<'_>) -> Vec<Violation> {
    let mut found = Vec::new();
    let has_endpoints = conf
        .pointer("/plugins/updater/endpoints")
        .and_then(Value::as_array)
        .is_some_and(|endpoints| !endpoints.is_empty());
    let has_pubkey = conf
        .pointer("/plugins/updater/pubkey")
        .and_then(Value::as_str)
        .is_some_and(|key| !key.trim().is_empty());
    if has_endpoints && !has_pubkey {
        found.push(at(
            &["plugins", "updater", "endpoints"],
            "plugins.updater has endpoints but no pubkey".to_owned(),
            PUBKEY_FIX,
        ));
    }
    if conf.pointer("/bundle/createUpdaterArtifacts") == Some(&Value::from("v1Compatible")) {
        found.push(at(
            &["bundle", "createUpdaterArtifacts"],
            "bundle.createUpdaterArtifacts is \"v1Compatible\"".to_owned(),
            V1_FIX,
        ));
    }
    found
}

/// The file names of every platform config Tauri would merge over `tauri.conf.json`.
fn platform_configs() -> Vec<String> {
    PLATFORMS
        .iter()
        .flat_map(|platform| {
            [
                format!("tauri.{platform}.conf.json"),
                format!("tauri.{platform}.conf.json5"),
                format!("Tauri.{platform}.toml"),
            ]
        })
        .collect()
}

/// The violation for the platform config at `path`, which exists.
fn platform_violation(path: &str) -> Violation {
    violation(
        path,
        1,
        format!("platform config `{path}` overrides tauri.conf.json"),
        PLATFORM_FIX,
    )
}

/// The key path of every object key, at any depth, whose name starts with `dangerous`, in
/// document order. Array elements add no key to the path.
fn dangerous_keys(value: &Value) -> Vec<Vec<String>> {
    let mut found = Vec::new();
    collect_dangerous(value, &mut Vec::new(), &mut found);
    found
}

fn collect_dangerous(value: &Value, path: &mut Vec<String>, found: &mut Vec<Vec<String>>) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                path.push(key.clone());
                if key.starts_with("dangerous") {
                    found.push(path.clone());
                }
                collect_dangerous(child, path, found);
                path.pop();
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_dangerous(item, path, found);
            }
        }
        _ => {}
    }
}

/// `build.rs` at `path` must declare an `AppManifest` and must not move the capabilities folder.
/// Comments are stripped first (`checks::strip_comments`), so a commented-out call does not count.
/// A missing file counts as no manifest.
pub(crate) fn build_rs_violations(path: &str, text: Option<&str>) -> Vec<Violation> {
    let code = text.map(checks::strip_comments).unwrap_or_default();
    let mut found = Vec::new();
    if !code.contains("app_manifest(") || !code.contains("AppManifest::new()") {
        found.push(violation(
            path,
            1,
            "build.rs declares no AppManifest",
            APP_MANIFEST_FIX,
        ));
    }
    if let Some(index) = code
        .lines()
        .position(|line| line.contains("capabilities_path_pattern("))
    {
        found.push(violation(
            path,
            checks::line_at(index),
            "build.rs calls `capabilities_path_pattern(`",
            PATH_PATTERN_FIX,
        ));
    }
    found
}

#[cfg(test)]
mod tests;
