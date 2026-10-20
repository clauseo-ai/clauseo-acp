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
//! In `build.rs`, parsed with syn: an `AppManifest` is declared (`app_manifest(` and
//! `AppManifest::new()` are called), and no config is passed in code (a `config_path(...)` call,
//! or a string naming `TAURI_CONFIG`), which Tauri would read instead of, or merge over, the
//! checked file. Platform config names match without regard to case, as a macOS disk does.
//! Moving the capabilities folder with `capabilities_path_pattern` is banned in clippy.toml's
//! `disallowed-methods`, not here: clippy resolves the path, so a fn pointer or a macro cannot
//! hide the call. In the desktop crate's other Rust files, no `generate_context!` call has
//! arguments, and `.cargo/config.toml` sets no `TAURI_CONFIG` in `[env]` (`sources.rs`). Each
//! failing setting points
//! at its key's line, found under its parent object (`json_text::key_line`), or at line 1.
//! The pure functions take file contents and paths; `run` reads the files from the repo and counts
//! the ones it found: `tauri.conf.json`, `build.rs`, each platform config, each other Rust file
//! of the desktop crate and `.cargo/config.toml`.
//!
//! Left open: a manifest written only in a `build.rs` function that is never called passes the
//! `AppManifest` rule (it would need reachability), and a name built by `concat!("TAURI_",
//! "CONFIG")` passes the `TAURI_CONFIG` rule.

mod fix;
pub(crate) use fix::Fix;

mod sources;

use crate::checks::json_text::key_line;
use crate::checks::{self, Inspection, Noun, StaticCheck, violation};
use crate::violation::Violation;
use crate::workspace::Context;
use proc_macro2::{Delimiter, TokenStream, TokenTree};
use serde_json::Value;
use std::fs;
use std::path::Path;
use syn::ext::IdentExt;

/// The files this check reads, inside the desktop crate's directory.
const TAURI_CONF: &str = "tauri.conf.json";
const BUILD_RS: &str = "build.rs";

/// The variable whose JSON tauri-build and `generate_context!` merge over `tauri.conf.json`.
const ENV_CONFIG: &str = "TAURI_CONFIG";

/// The platforms Tauri merges a platform config for, over `tauri.conf.json`.
const PLATFORMS: [&str; 5] = ["macos", "windows", "linux", "ios", "android"];

/// The one CSP `app.security.csp` may hold. The policy is decided, so a change to it is an edit
/// here too, which puts it in front of a reviewer.
pub(crate) const PINNED_CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data: clauseo-file: http://clauseo-file.localhost https://clauseo-file.localhost; font-src 'self'; connect-src ipc: http://ipc.localhost https://ipc.localhost; worker-src 'self'; frame-src 'self'; child-src 'self'; object-src 'none'; media-src 'none'; manifest-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'; require-trusted-types-for 'script'; trusted-types clauseo-sanitize";

/// Fails on every unsafe setting in `tauri.conf.json` and on a `build.rs` without an `AppManifest`.
pub(crate) const CHECK: StaticCheck = StaticCheck {
    name: "tauri-config",
    counts: Noun {
        one: "file",
        many: "files",
    },
    place: || {
        format!(
            "in the desktop crate ({TAURI_CONF}, {BUILD_RS} and its other Rust files) and {}",
            checks::CARGO_CONFIG
        )
    },
    source: file!(),
    run,
    #[cfg(test)]
    planted: tests::planted::PLANTED,
};

fn run(ctx: &Context) -> Inspection {
    let desktop = match checks::desktop_dir(ctx) {
        Ok(dir) => dir,
        Err(stopped) => return stopped,
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
    let platform: Vec<String> = platform_configs_in(&ctx.root.join(desktop))
        .into_iter()
        .map(|name| checks::join(desktop, &name))
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
    let (sources_read, in_sources) = sources::source_violations(&ctx.root, desktop, &build);
    read += sources_read;
    found.extend(in_sources);
    match checks::read_repo_file(&ctx.root, checks::CARGO_CONFIG) {
        Ok(Some(text)) => {
            read += 1;
            found.extend(sources::env_violations(checks::CARGO_CONFIG, &text));
        }
        Ok(None) => {}
        Err(violation) => found.push(violation),
    }
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
            Fix::ConfMissing,
        )];
    };
    let conf: Value = match serde_json::from_str(text) {
        Ok(conf) => conf,
        Err(error) => {
            return vec![violation(
                path,
                1,
                format!("`{path}` is not valid JSON: {error}"),
                Fix::ConfInvalid,
            )];
        }
    };
    let at = |keys: &[&str], message: String, fix: Fix| {
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
            Fix::Dangerous,
        ));
    }
    found
}

/// Builds the violation for a message and fix at the line of the key on a path.
type At<'a> = dyn Fn(&[&str], String, Fix) -> Violation + 'a;

/// The global object, the frozen prototype, config windows and the asset protocol.
fn app_violations(conf: &Value, at: &At<'_>) -> Vec<Violation> {
    let mut found = Vec::new();
    if conf.pointer("/app/withGlobalTauri") != Some(&Value::Bool(false)) {
        found.push(at(
            &["app", "withGlobalTauri"],
            "app.withGlobalTauri must be false".to_owned(),
            Fix::GlobalTauri,
        ));
    }
    if conf.pointer("/app/security/freezePrototype") != Some(&Value::Bool(true)) {
        found.push(at(
            &["app", "security", "freezePrototype"],
            "app.security.freezePrototype must be true".to_owned(),
            Fix::FreezePrototype,
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
            Fix::Windows,
        ));
    }
    if conf.pointer("/app/security/assetProtocol/enable") == Some(&Value::Bool(true)) {
        found.push(at(
            &["app", "security", "assetProtocol", "enable"],
            "app.security.assetProtocol.enable is true".to_owned(),
            Fix::AssetProtocol,
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
        Fix::CspPinned,
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
        Fix::Frontend,
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
            Fix::Pubkey,
        ));
    }
    if conf.pointer("/bundle/createUpdaterArtifacts") == Some(&Value::from("v1Compatible")) {
        found.push(at(
            &["bundle", "createUpdaterArtifacts"],
            "bundle.createUpdaterArtifacts is \"v1Compatible\"".to_owned(),
            Fix::V1Compatible,
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

/// The names in `dir` of the platform configs, matched without regard to ASCII case: a disk that
/// ignores case (macOS, Windows) hands `TAURI.MACOS.CONF.JSON` to Tauri too, while a Linux CI
/// would find no file of the exact name. In the order of `platform_configs`; none when `dir`
/// cannot be listed.
fn platform_configs_in(dir: &Path) -> Vec<String> {
    let names: Vec<String> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect();
    platform_configs()
        .iter()
        .flat_map(|wanted| {
            names
                .iter()
                .filter(move |name| name.eq_ignore_ascii_case(wanted))
                .cloned()
        })
        .collect()
}

/// The violation for the platform config at `path`, which exists.
fn platform_violation(path: &str) -> Violation {
    violation(
        path,
        1,
        format!("platform config `{path}` overrides tauri.conf.json"),
        Fix::Platform,
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

/// `build.rs` at `path` must declare an `AppManifest` (call `app_manifest(...)` and
/// `AppManifest::new()`). The file is parsed with syn and its tokens are walked, macro arguments
/// included, so a call written only in a comment or a string does not count. A missing file counts
/// as no manifest; a file that is not valid Rust fails at the parse error's line.
pub(crate) fn build_rs_violations(path: &str, text: Option<&str>) -> Vec<Violation> {
    let mut calls = BuildCalls::default();
    if let Some(text) = text {
        match parse_rust(text) {
            Ok(tokens) => calls.find(tokens),
            Err(error) => {
                let line = u32::try_from(error.span().start().line).unwrap_or(1).max(1);
                return vec![violation(
                    path,
                    line,
                    format!("build.rs is not valid Rust: {error}"),
                    Fix::BuildRsSyntax,
                )];
            }
        }
    }
    let mut found = Vec::new();
    if !calls.app_manifest || !calls.manifest_new {
        found.push(violation(
            path,
            1,
            "build.rs declares no AppManifest",
            Fix::AppManifest,
        ));
    }
    for (line, message) in calls.config_in_code {
        found.push(violation(path, line, message, Fix::ConfigInCode));
    }
    found
}

/// The tokens of `text`, once syn has parsed it as a Rust file.
fn parse_rust(text: &str) -> syn::Result<TokenStream> {
    let tokens: TokenStream = text.parse()?;
    syn::parse2::<syn::File>(tokens.clone())?;
    Ok(tokens)
}

/// The calls in `build.rs` that its rule looks for.
#[derive(Default)]
struct BuildCalls {
    /// `app_manifest(...)` is called.
    app_manifest: bool,
    /// `AppManifest::new()` is called.
    manifest_new: bool,
    /// The line and message of each config passed in code: a `config_path(...)` call, or a
    /// literal naming `TAURI_CONFIG`, which tauri-build and `generate_context!` merge over
    /// `tauri.conf.json` (set with `env::set_var` or `cargo:rustc-env=`).
    config_in_code: Vec<(u32, String)>,
}

impl BuildCalls {
    /// Records the calls in `tokens`, inside every group (macro arguments too). A call is a name,
    /// raw or not, followed by a parenthesized group and not after `fn`.
    fn find(&mut self, tokens: TokenStream) {
        let trees: Vec<TokenTree> = tokens.into_iter().collect();
        for (index, tree) in trees.iter().enumerate() {
            let ident = match tree {
                TokenTree::Group(group) => {
                    self.find(group.stream());
                    continue;
                }
                TokenTree::Ident(ident) => ident.unraw(),
                TokenTree::Literal(literal) => {
                    if literal.to_string().contains(ENV_CONFIG) {
                        let line = line_of(literal.span());
                        let message = format!("build.rs names `{ENV_CONFIG}`");
                        self.config_in_code.push((line, message));
                    }
                    continue;
                }
                TokenTree::Punct(_) => continue,
            };
            let args = match trees.get(index + 1) {
                Some(TokenTree::Group(group)) if group.delimiter() == Delimiter::Parenthesis => {
                    group.stream()
                }
                _ => continue,
            };
            let before = |back: usize| index.checked_sub(back).and_then(|at| trees.get(at));
            if matches!(before(1), Some(TokenTree::Ident(word)) if word == "fn") {
                continue;
            }
            if ident == "app_manifest" {
                self.app_manifest = true;
            } else if ident == "config_path" {
                let line = line_of(tree.span());
                let message = "build.rs calls `config_path`".to_owned();
                self.config_in_code.push((line, message));
            } else if ident == "new" && args.is_empty() && after_app_manifest_path(&before) {
                self.manifest_new = true;
            }
        }
    }
}

/// The 1-based line where `span` starts.
fn line_of(span: proc_macro2::Span) -> u32 {
    u32::try_from(span.start().line).unwrap_or(1).max(1)
}

/// True when the three tokens before a name are `AppManifest`, `:` and `:`.
fn after_app_manifest_path<'a>(before: &impl Fn(usize) -> Option<&'a TokenTree>) -> bool {
    let colon = |back| matches!(before(back), Some(TokenTree::Punct(p)) if p.as_char() == ':');
    colon(1)
        && colon(2)
        && matches!(before(3), Some(TokenTree::Ident(name)) if name.unraw() == "AppManifest")
}

#[cfg(test)]
mod tests;
