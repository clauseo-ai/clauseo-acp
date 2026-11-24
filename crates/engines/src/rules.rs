//! The accept rules on a parsed list: official names only, the app listed, the rollback rule
//! and never older than the bundled pins.

use semver::Version;
use serde_json::{Map, Value};

use crate::list::EngineList;
use crate::{Context, Error};

/// The two engines: `package_json.dependencies` has exactly these keys.
pub const DEPENDENCIES: [&str; 2] = [
    "@agentclientprotocol/claude-agent-acp",
    "@agentclientprotocol/codex-acp",
];

/// The overrides `package_json.overrides` may have: codex-acp's native binary.
pub const OVERRIDES: [&str; 1] = ["@openai/codex"];

/// Where every locked package must resolve from.
pub const NPM_REGISTRY: &str = "https://registry.npmjs.org/";

/// The fields an engines pin file has (those of `crates/limits/tests/live/package.json`). Any
/// other, such as `scripts`, which `npm ci` would run, is refused.
const PACKAGE_JSON_FIELDS: [&str; 7] = [
    "name",
    "version",
    "private",
    "description",
    "license",
    "dependencies",
    "overrides",
];

/// Every rule after the shape, in the crate docs' order.
pub(crate) fn check(list: &EngineList, cx: &Context<'_>) -> Result<(), Error> {
    let package_json = list
        .package_json()
        .as_object()
        .ok_or(Error::PackageJsonField)?;
    if package_json
        .keys()
        .any(|key| !PACKAGE_JSON_FIELDS.contains(&key.as_str()))
    {
        return Err(Error::PackageJsonField);
    }
    let dependencies = package_json
        .get("dependencies")
        .and_then(Value::as_object)
        .ok_or(Error::Dependencies)?;
    if dependencies.len() != DEPENDENCIES.len()
        || !DEPENDENCIES
            .iter()
            .all(|name| dependencies.contains_key(*name))
    {
        return Err(Error::Dependencies);
    }
    let empty = Map::new();
    let overrides = match package_json.get("overrides") {
        None => &empty,
        Some(value) => value.as_object().ok_or(Error::Overrides)?,
    };
    if overrides
        .iter()
        .any(|(key, value)| !OVERRIDES.contains(&key.as_str()) || !value.is_string())
    {
        return Err(Error::Overrides);
    }
    lock(list.package_lock())?;
    if !list.apps().iter().any(|app| app == cx.app_version) {
        return Err(Error::AppNotListed);
    }
    let floor = cx.bundled_seq.max(cx.in_use_seq.unwrap_or(0));
    if list.seq() <= floor {
        return Err(Error::NotNewer {
            seq: list.seq(),
            floor,
        });
    }
    not_older(dependencies, overrides, cx.bundled_package_json)
}

/// `lockfileVersion` 3, and every package but the root locked to the registry by sha512.
fn lock(lock: &Value) -> Result<(), Error> {
    if lock.get("lockfileVersion").and_then(Value::as_u64) != Some(3) {
        return Err(Error::Lockfile);
    }
    let packages = lock
        .get("packages")
        .and_then(Value::as_object)
        .ok_or(Error::Lockfile)?;
    for (path, entry) in packages {
        if path.is_empty() {
            continue;
        }
        let integrity = entry.get("integrity").and_then(Value::as_str);
        if !integrity.is_some_and(|text| text.starts_with("sha512-")) {
            return Err(Error::Integrity);
        }
        if let Some(resolved) = entry.get("resolved")
            && !resolved
                .as_str()
                .is_some_and(|text| text.starts_with(NPM_REGISTRY))
        {
            return Err(Error::Resolved);
        }
    }
    Ok(())
}

/// Each dependency, and each override the bundle pins, at or above the bundled version.
fn not_older(
    dependencies: &Map<String, Value>,
    overrides: &Map<String, Value>,
    bundled: &Value,
) -> Result<(), Error> {
    for name in DEPENDENCIES {
        let ours = bundled_version(bundled, "dependencies", name)?.ok_or(Error::BundledVersion)?;
        compare(dependencies.get(name), &ours)?;
    }
    for name in OVERRIDES {
        if let Some(ours) = bundled_version(bundled, "overrides", name)? {
            compare(Some(overrides.get(name).ok_or(Error::Overrides)?), &ours)?;
        }
    }
    Ok(())
}

/// The bundled `package.json`'s version of `name` under `section`: `None` when absent.
fn bundled_version(bundled: &Value, section: &str, name: &str) -> Result<Option<Version>, Error> {
    let Some(value) = bundled.get(section).and_then(|section| section.get(name)) else {
        return Ok(None);
    };
    let text = value.as_str().ok_or(Error::BundledVersion)?;
    Version::parse(text)
        .map(Some)
        .ok()
        .ok_or(Error::BundledVersion)
}

/// The listed version, plain semver and at or above `ours`.
fn compare(listed: Option<&Value>, ours: &Version) -> Result<(), Error> {
    let text = listed.and_then(Value::as_str).ok_or(Error::Version)?;
    let listed = Version::parse(text).ok().ok_or(Error::Version)?;
    if listed < *ours {
        return Err(Error::OlderThanBundled);
    }
    Ok(())
}
