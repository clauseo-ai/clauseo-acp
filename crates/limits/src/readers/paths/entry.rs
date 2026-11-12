//! The files node may load for `import("@anthropic-ai/claude-agent-sdk")`, read from the SDK's
//! `package.json` by node's rules for a package's entry.
//!
//! With `exports`, its `"."` entry names the file, through the conditions node matches for an
//! `import`. `serde_json` keeps an object's key order in some builds and sorts it in others,
//! and node takes the first matching condition, so every file a matching condition names
//! counts: one that node would skip still has to pass. An `exports` array (node's fallback
//! list) is refused rather than followed. Without `exports`, the file is `main` as written, or
//! `index.js` with no `main`; node's guessed extensions are not tried. A `--conditions` flag in
//! the agent's `NODE_OPTIONS` would add conditions this does not follow.

use std::fs::File;
use std::io::Read as _;
use std::path::{Path, PathBuf};

use serde_json::Value;

/// The conditions node matches for an `import` when started with no `--conditions` flag.
const IMPORT_CONDITIONS: [&str; 4] = ["node", "import", "module-sync", "default"];
/// `package.json` is read up to this many bytes; a larger one is refused.
const MAX_PACKAGE_BYTES: u64 = 1024 * 1024;

/// The files node may load as the entry of the package whose `package.json` is `package`, each
/// under the package's folder. `package` has passed the no-link walk. Errs with the rule broken.
pub(super) fn entry_files(package: &Path) -> Result<Vec<PathBuf>, String> {
    let dir = package.parent().unwrap_or(package);
    let value: Value = serde_json::from_slice(&read_capped(package)?)
        .map_err(|err| format!("{} is not JSON: {err}", package.display()))?;
    let (targets, exported) = match value.get("exports") {
        Some(exports) => (exported_files(exports), true),
        None => (main_file(value.get("main")), false),
    };
    let targets = targets.map_err(|what| format!("{} {what}", package.display()))?;
    targets
        .into_iter()
        .map(|target| {
            parts(target, exported)
                .map(|parts| {
                    parts
                        .iter()
                        .fold(dir.to_owned(), |path, part| path.join(part))
                })
                .ok_or_else(|| {
                    format!(
                        "{} names the entry {target:?}, which leaves the package or is not a \
                         plain relative path",
                        package.display()
                    )
                })
        })
        .collect()
}

/// `package`'s bytes, refused past [`MAX_PACKAGE_BYTES`].
fn read_capped(package: &Path) -> Result<Vec<u8>, String> {
    let fail = |err: std::io::Error| format!("{} does not read: {err}", package.display());
    // A swap after the walk is left open: whoever can write the install folder can change the
    // adapter itself anyway.
    let mut bytes = Vec::new();
    File::open(package)
        .map_err(fail)?
        .take(MAX_PACKAGE_BYTES.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(fail)?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_PACKAGE_BYTES {
        return Err(format!(
            "{} is larger than {MAX_PACKAGE_BYTES} bytes",
            package.display()
        ));
    }
    Ok(bytes)
}

/// The entry without `exports`: `main`, or `index.js` when there is none.
fn main_file(main: Option<&Value>) -> Result<Vec<&str>, &'static str> {
    match main {
        None => Ok(vec!["index.js"]),
        Some(Value::String(main)) => Ok(vec![main]),
        Some(_) => Err("has a main that is not a string"),
    }
}

/// Every file the `"."` entry of `exports` names under [`IMPORT_CONDITIONS`].
fn exported_files(exports: &Value) -> Result<Vec<&str>, &'static str> {
    let dot = match exports {
        Value::Object(map) if map.keys().any(|key| key.starts_with('.')) => {
            if !map.keys().all(|key| key.starts_with('.')) {
                return Err("mixes subpaths and conditions in exports");
            }
            map.get(".").ok_or("exports no \".\" entry")?
        }
        other => other,
    };
    let mut files = Vec::new();
    collect(dot, &mut files)?;
    if files.is_empty() {
        return Err("exports no file for an import");
    }
    Ok(files)
}

/// Adds the files `target` names under [`IMPORT_CONDITIONS`] to `files`. A `null` names none.
fn collect<'a>(target: &'a Value, files: &mut Vec<&'a str>) -> Result<(), &'static str> {
    match target {
        Value::String(file) => files.push(file),
        Value::Object(conditions) => {
            for (condition, target) in conditions {
                if IMPORT_CONDITIONS.contains(&condition.as_str()) {
                    collect(target, files)?;
                }
            }
        }
        Value::Null => {}
        Value::Array(_) => return Err("has an exports array, which this check does not follow"),
        Value::Bool(_) | Value::Number(_) => return Err("has an export that is not a file"),
    }
    Ok(())
}

/// The parts of `target` below the package folder. An exported target must start with `./`;
/// `main` may leave it off. No target may hold a `\` and no part may be empty, `.`, `..` or
/// `node_modules`: node refuses each in an export (`main` gets the same rule).
fn parts(target: &str, exported: bool) -> Option<Vec<&str>> {
    let rest = match target.strip_prefix("./") {
        Some(rest) => rest,
        None if exported => return None,
        None => target,
    };
    if rest.contains('\\') {
        return None;
    }
    let parts: Vec<&str> = rest.split('/').collect();
    parts
        .iter()
        .all(|part| {
            !part.is_empty()
                && *part != "."
                && *part != ".."
                && !part.eq_ignore_ascii_case("node_modules")
        })
        .then_some(parts)
}
