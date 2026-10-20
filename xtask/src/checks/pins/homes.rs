//! Reads the pinned versions from their homes: `channel` in `rust-toolchain.toml`, and
//! `packageManager` and `engines.node` in `ui/package.json`. A value that cannot be read is a
//! failure with no line (the value is missing, so there is no line to point at), and the rules
//! that compare copies with it skip it.

use super::{Fix, PACKAGE_JSON, TOOLCHAIN, Unread, is_version};
use crate::checks::{Failure, Inspection};
use std::fs;
use std::io;
use std::path::Path;

/// The versions the homes pin, each `None` when it could not be read.
#[derive(Debug, Default)]
pub(super) struct Homes {
    /// `channel`, e.g. `1.2.3`.
    pub rust: Option<String>,
    /// The version in `packageManager`, without `pnpm@` and any `+<hash>`.
    pub pnpm: Option<String>,
    /// The version in `engines.node`, without its operator (`>=`).
    pub node: Option<String>,
}

/// Reads every home, adding a failure to `found` for each value it could not read.
pub(super) fn read(root: &Path, found: &mut Inspection) -> Homes {
    let mut keep = |unread: Unread, what: &str, file: &str, read: Result<String, String>| {
        read.map_err(|cause| {
            found.failures.push(Failure {
                message: format!("could not read {what} from {file}: {cause}"),
                fix: Fix::Unread(unread).into(),
            });
        })
        .ok()
    };
    let rust = keep(
        Unread::Channel,
        "the Rust version",
        TOOLCHAIN,
        rust_channel(root),
    );
    let package = package_json(root);
    let pnpm = keep(
        Unread::PackageManager,
        "the pnpm version",
        PACKAGE_JSON,
        package.clone().and_then(|json| pnpm_version(&json)),
    );
    let node = keep(
        Unread::EnginesNode,
        "the minimum Node",
        PACKAGE_JSON,
        package.and_then(|json| node_version(&json)),
    );
    Homes { rust, pnpm, node }
}

/// `root/rel` as text, or why it could not be read.
fn text(root: &Path, rel: &str) -> Result<String, String> {
    fs::read_to_string(root.join(rel)).map_err(|error| match error.kind() {
        io::ErrorKind::NotFound => "the file does not exist".to_owned(),
        _ => error.to_string(),
    })
}

/// `channel` under `[toolchain]`.
fn rust_channel(root: &Path) -> Result<String, String> {
    let text = text(root, TOOLCHAIN)?;
    let table: toml::Table =
        toml::from_str(&text).map_err(|error| format!("it is not TOML: {}", error.message()))?;
    let channel = table
        .get("toolchain")
        .and_then(|toolchain| toolchain.get("channel"))
        .and_then(toml::Value::as_str)
        .ok_or("it has no `channel` string under `[toolchain]`")?;
    if is_version(channel) {
        Ok(channel.to_owned())
    } else {
        Err(format!(
            "`channel = \"{channel}\"` is not a version such as 1.2.3"
        ))
    }
}

/// `ui/package.json`, parsed.
fn package_json(root: &Path) -> Result<serde_json::Value, String> {
    let text = text(root, PACKAGE_JSON)?;
    serde_json::from_str(&text).map_err(|error| format!("it is not JSON: {error}"))
}

/// The version in `"packageManager": "pnpm@<version>[+<hash>]"`.
fn pnpm_version(json: &serde_json::Value) -> Result<String, String> {
    let field = json
        .get("packageManager")
        .and_then(serde_json::Value::as_str)
        .ok_or("it has no `packageManager` string")?;
    let version = field
        .strip_prefix("pnpm@")
        .map(|rest| rest.split('+').next().unwrap_or(rest))
        .filter(|version| is_version(version));
    version
        .map(str::to_owned)
        .ok_or_else(|| format!("`packageManager` is `{field}`, not `pnpm@<major>.<minor>.<patch>`"))
}

/// The version in `engines.node`, after its operator: `1.2.3` of `>=1.2.3`.
fn node_version(json: &serde_json::Value) -> Result<String, String> {
    let field = json
        .get("engines")
        .and_then(|engines| engines.get("node"))
        .and_then(serde_json::Value::as_str)
        .ok_or("it has no `engines.node` string")?;
    let version = field
        .trim_start_matches(['>', '=', '^', '~', 'v', ' '])
        .split([' ', '|', '<'])
        .next()
        .filter(|version| is_version(version));
    version
        .map(str::to_owned)
        .ok_or_else(|| format!("`engines.node` is `{field}`, which starts with no version"))
}
