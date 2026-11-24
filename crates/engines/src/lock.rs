//! The lock's rules: a v3 `package-lock.json` whose every package is a plain registry tarball
//! under `node_modules`, locked by one sha512, depending only on registry specs, with no install
//! script, and whose root, engines and codex packages match the pins at every depth.

use std::collections::BTreeSet;

use serde_json::{Map, Value};

use crate::Error;
use crate::npm::{installed_name, is_package_name, is_registry_spec, is_sha512, is_version_text};

/// Where every locked package must resolve from.
pub const NPM_REGISTRY: &str = "https://registry.npmjs.org/";

/// The codex override's package: its platform builds are installed as aliases of it.
const CODEX: &str = "@openai/codex";

/// The platforms codex's aliases are published for, as `node_modules/@openai/codex-<platform>`
/// at version `<pin>-<platform>`.
const CODEX_PLATFORMS: [&str; 6] = [
    "darwin-arm64",
    "darwin-x64",
    "linux-arm64",
    "linux-x64",
    "win32-arm64",
    "win32-x64",
];

/// The fields of an entry that hold dependency specs npm resolves while building the tree.
const SPEC_FIELDS: [&str; 3] = ["dependencies", "optionalDependencies", "peerDependencies"];

/// Checks `lock` against the pins `dependencies` and `overrides` (already checked to be the
/// official names, each an exact version):
/// - `lockfileVersion` is 3 and `packages` is an object with a root `""` whose `dependencies`
///   equal `dependencies`;
/// - every other key is `node_modules/<name>(/node_modules/<name>)*` with lower-case npm names,
///   and no two keys are equal ignoring case;
/// - every other entry passes [`package`];
/// - `node_modules/<dep>` exists for each dependency and override.
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
    let pins: Vec<(&str, &str)> = dependencies
        .iter()
        .chain(overrides)
        .filter_map(|(name, version)| Some((name.as_str(), version.as_str()?)))
        .collect();
    let mut folded = BTreeSet::new();
    for (path, entry) in packages {
        if path.is_empty() {
            continue;
        }
        let last = installed_name(path).ok_or(Error::LockPath)?;
        if !folded.insert(path.to_ascii_lowercase()) {
            return Err(Error::LockPath);
        }
        package(entry, last, &pins)?;
    }
    for (name, _) in &pins {
        if !packages.contains_key(&format!("node_modules/{name}")) {
            return Err(Error::LockMismatch);
        }
    }
    Ok(())
}

/// One entry other than the root, installed at a path whose last name is `last`:
/// - no `link`, `inBundle` or `hasInstallScript`;
/// - a `name` only on a codex platform alias (`@openai/codex` at
///   `node_modules/@openai/codex-<platform>`);
/// - a string `version`; a package named in `pins` at exactly its pin (`<pin>-<platform>` for a
///   codex alias), wherever it is installed;
/// - every spec in `dependencies`, `optionalDependencies` and `peerDependencies` a registry
///   spec ([`is_registry_spec`]);
/// - an `integrity` that is exactly one sha512, and a `resolved` that is exactly
///   `https://registry.npmjs.org/<pkg>/-/<base>-<version>.tgz`, `<pkg>` being the `name` when
///   there is one else `last`, and `<base>` `<pkg>` without its scope.
fn package(entry: &Value, last: &str, pins: &[(&str, &str)]) -> Result<(), Error> {
    let entry = entry.as_object().ok_or(Error::LockEntry)?;
    if ["link", "inBundle", "hasInstallScript"]
        .iter()
        .any(|field| entry.contains_key(*field))
    {
        return Err(Error::LockEntry);
    }
    let version = entry
        .get("version")
        .and_then(Value::as_str)
        .ok_or(Error::LockEntry)?;
    let (pkg, platform) = match entry.get("name") {
        None => (last, None),
        Some(name) => {
            let platform = last
                .strip_prefix(CODEX)
                .and_then(|rest| rest.strip_prefix('-'))
                .filter(|platform| CODEX_PLATFORMS.contains(platform));
            match (name.as_str(), platform) {
                (Some(CODEX), Some(platform)) => (CODEX, Some(platform)),
                _ => return Err(Error::LockEntry),
            }
        }
    };
    if !is_package_name(pkg) {
        return Err(Error::LockEntry);
    }
    if let Some((_, pin)) = pins.iter().find(|(name, _)| *name == pkg) {
        let pinned = match platform {
            None => version == *pin,
            Some(platform) => {
                version
                    .strip_prefix(*pin)
                    .and_then(|rest| rest.strip_prefix('-'))
                    == Some(platform)
            }
        };
        if !pinned {
            return Err(Error::LockMismatch);
        }
    } else if [CODEX, crate::DEPENDENCIES[0], crate::DEPENDENCIES[1]].contains(&pkg) {
        // A pinned name with no pin in this list (codex with no override): nothing to match.
        return Err(Error::LockMismatch);
    }
    for field in SPEC_FIELDS {
        let Some(specs) = entry.get(field) else {
            continue;
        };
        let specs = specs.as_object().ok_or(Error::LockSpec)?;
        if specs
            .values()
            .any(|spec| !spec.as_str().is_some_and(is_registry_spec))
        {
            return Err(Error::LockSpec);
        }
    }
    let integrity = entry.get("integrity").and_then(Value::as_str);
    if !integrity.is_some_and(is_sha512) {
        return Err(Error::Integrity);
    }
    if !is_version_text(version) {
        return Err(Error::Resolved);
    }
    let base = pkg.rsplit('/').next().unwrap_or(pkg);
    let expected = format!("{NPM_REGISTRY}{pkg}/-/{base}-{version}.tgz");
    if entry.get("resolved").and_then(Value::as_str) != Some(expected.as_str()) {
        return Err(Error::Resolved);
    }
    Ok(())
}
