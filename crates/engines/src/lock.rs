//! The lock's rules: a v3 `package-lock.json` whose every package is a plain registry tarball
//! under `node_modules`, locked by one sha512, and whose root and adapters match the pins.

use serde_json::{Map, Value};

use crate::Error;

/// Where every locked package must resolve from.
pub const NPM_REGISTRY: &str = "https://registry.npmjs.org/";

/// npm's longest package name.
const MAX_NAME: usize = 214;

/// Checks `lock` against the pins `dependencies` and `overrides` (already checked to be the
/// official names, each an exact version):
/// - `lockfileVersion` is 3 and `packages` is an object with a root `""` whose `dependencies`
///   equal `dependencies`;
/// - every other key is `node_modules/<name>(/node_modules/<name>)*`, each `<name>` a valid npm
///   name, optionally `@scope/`d;
/// - every other entry has no `link` or `inBundle`, a string `version`, an `integrity` that is
///   exactly `sha512-` and 86 base64 characters and `==`, and a `resolved` that is exactly
///   `https://registry.npmjs.org/<pkg>/-/<base>-<version>.tgz`, `<pkg>` being the entry's
///   `name` when it has one (an alias, such as codex's platform packages) else the key's last
///   name, and `<base>` `<pkg>` without its scope;
/// - `node_modules/<dep>` is at each dependency's pinned version, and `node_modules/<name>` at
///   each override's.
pub(crate) fn check(
    lock: &Value,
    dependencies: &Map<String, Value>,
    overrides: &Map<String, Value>,
) -> Result<(), Error> {
    if lock.get("lockfileVersion").and_then(Value::as_u64) != Some(3) {
        return Err(Error::Lockfile);
    }
    let packages = lock
        .get("packages")
        .and_then(Value::as_object)
        .ok_or(Error::Lockfile)?;
    let root = packages.get("").ok_or(Error::LockMismatch)?;
    if root.get("dependencies").and_then(Value::as_object) != Some(dependencies) {
        return Err(Error::LockMismatch);
    }
    for (path, entry) in packages {
        if path.is_empty() {
            continue;
        }
        let last = installed_name(path).ok_or(Error::LockPath)?;
        package(entry, last)?;
    }
    for (name, version) in dependencies.iter().chain(overrides) {
        let installed = packages
            .get(&format!("node_modules/{name}"))
            .and_then(|entry| entry.get("version"));
        if installed != Some(version) {
            return Err(Error::LockMismatch);
        }
    }
    Ok(())
}

/// One entry other than the root, installed at a path whose last name is `last`.
fn package(entry: &Value, last: &str) -> Result<(), Error> {
    let entry = entry.as_object().ok_or(Error::LockEntry)?;
    if entry.contains_key("link") || entry.contains_key("inBundle") {
        return Err(Error::LockEntry);
    }
    let version = entry
        .get("version")
        .and_then(Value::as_str)
        .ok_or(Error::LockEntry)?;
    let integrity = entry.get("integrity").and_then(Value::as_str);
    if !integrity.is_some_and(is_sha512) {
        return Err(Error::Integrity);
    }
    let pkg = match entry.get("name") {
        None => last,
        Some(name) => name
            .as_str()
            .filter(|name| is_package_name(name))
            .ok_or(Error::LockEntry)?,
    };
    let base = pkg.rsplit('/').next().unwrap_or(pkg);
    if !is_version_text(version) {
        return Err(Error::Resolved);
    }
    let expected = format!("{NPM_REGISTRY}{pkg}/-/{base}-{version}.tgz");
    if entry.get("resolved").and_then(Value::as_str) != Some(expected.as_str()) {
        return Err(Error::Resolved);
    }
    Ok(())
}

/// The last package name of a lock key `node_modules/<name>(/node_modules/<name>)*`, or `None`
/// when the key isn't that form.
fn installed_name(path: &str) -> Option<&str> {
    let mut rest = path.strip_prefix("node_modules/")?;
    loop {
        let (name, after) = split_name(rest)?;
        if after.is_empty() {
            return Some(name);
        }
        rest = after.strip_prefix("/node_modules/")?;
    }
}

/// Splits a leading package name (`name` or `@scope/name`) off `text`: the name and the rest.
fn split_name(text: &str) -> Option<(&str, &str)> {
    let first = text.find('/').unwrap_or(text.len());
    let end = if text.starts_with('@') {
        let after = text.get(first + 1..)?;
        first + 1 + after.find('/').unwrap_or(after.len())
    } else {
        first
    };
    let (name, rest) = text.split_at_checked(end)?;
    is_package_name(name).then_some((name, rest))
}

/// npm's rules, as a lock can carry them: `name` or `@scope/name`, at most 214 bytes, each part
/// non-empty, of ASCII letters, digits, `-`, `.`, `_` and `~`, not starting with `.` or `_`.
/// So no `..`, no absolute path, no backslash, no `:`.
fn is_package_name(name: &str) -> bool {
    if name.is_empty() || name.len() > MAX_NAME {
        return false;
    }
    let part = |part: &str| {
        !part.is_empty()
            && !part.starts_with(['.', '_'])
            && part
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~'))
    };
    match name.strip_prefix('@') {
        Some(scoped) => scoped
            .split_once('/')
            .is_some_and(|(scope, rest)| part(scope) && part(rest)),
        None => part(name),
    }
}

/// A version as it appears in a tarball's file name: semver's characters only.
fn is_version_text(version: &str) -> bool {
    !version.is_empty()
        && version
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'+'))
}

/// `sha512-`, then 86 base64 characters and `==`: one 64-byte digest, one token.
fn is_sha512(text: &str) -> bool {
    let Some(digest) = text.strip_prefix("sha512-") else {
        return false;
    };
    let Some(body) = digest.strip_suffix("==") else {
        return false;
    };
    body.len() == 86
        && body
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/')
}
