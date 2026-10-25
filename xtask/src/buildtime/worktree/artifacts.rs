//! What cargo's `--message-format=json` stdout says a build did: which units it rebuilt and how
//! many it reused, read from its `compiler-artifact` messages.

/// One `compiler-artifact` message of cargo's `--message-format=json` stdout.
struct Artifact {
    /// Its package's name.
    package: String,
    /// `<package> (<target kinds>)`.
    unit: String,
    /// Whether cargo reused it (`"fresh": true`).
    fresh: bool,
}

/// Each `compiler-artifact` message in cargo's `--message-format=json` stdout, in order.
fn artifacts(stdout: &str) -> Vec<Artifact> {
    let mut found = Vec::new();
    for line in stdout.lines().filter(|line| line.starts_with('{')) {
        let Ok(message) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let artifact = message.get("reason").and_then(|r| r.as_str()) == Some("compiler-artifact");
        let fresh = message.get("fresh").and_then(serde_json::Value::as_bool);
        let id = message.get("package_id").and_then(|id| id.as_str());
        let (true, Some(fresh), Some(id)) = (artifact, fresh, id) else {
            continue;
        };
        let package = package_name(id).to_owned();
        let kinds: Vec<&str> = message
            .pointer("/target/kind")
            .and_then(serde_json::Value::as_array)
            .map(|kinds| kinds.iter().filter_map(|kind| kind.as_str()).collect())
            .unwrap_or_default();
        let unit = format!("{package} ({})", kinds.join(", "));
        found.push(Artifact {
            package,
            unit,
            fresh,
        });
    }
    found
}

/// The units cargo rebuilt (`"fresh": false`) whose package `keep` accepts, once each, in the
/// order cargo built them.
fn rebuilt(artifacts: &[Artifact], keep: impl Fn(&str) -> bool) -> Vec<&str> {
    let mut units: Vec<&str> = Vec::new();
    for artifact in artifacts.iter().filter(|artifact| !artifact.fresh) {
        if keep(&artifact.package) && !units.contains(&artifact.unit.as_str()) {
            units.push(&artifact.unit);
        }
    }
    units
}

/// The packages other than `package` that cargo rebuilt, from its `--message-format=json` stdout:
/// each `compiler-artifact` message with `"fresh": false`, named `<package> (<target kinds>)`, once
/// each, in the order cargo built them.
pub(crate) fn rebuilt_besides(stdout: &str, package: &str) -> Vec<String> {
    let found = artifacts(stdout);
    rebuilt(&found, |name| name != package)
        .into_iter()
        .map(str::to_owned)
        .collect()
}

/// What a build's `--message-format=json` stdout says cargo did: `first build: rebuilt <units>;
/// fresh: <n>`, the units as `rebuilt_besides` names them (`nothing` when there is none), and the
/// number of `compiler-artifact` messages it reused.
pub(crate) fn built_line(stdout: &str) -> String {
    let found = artifacts(stdout);
    let units = rebuilt(&found, |_| true);
    let fresh = found.iter().filter(|artifact| artifact.fresh).count();
    let units = if units.is_empty() {
        "nothing".to_owned()
    } else {
        units.join(", ")
    };
    format!("first build: rebuilt {units}; fresh: {fresh}")
}

/// The package name in a cargo package id: `<source>#<name>@<version>`, or `<source>#<version>`
/// when the name is the last part of the source's path (cargo 1.99, as pinned). An id in any
/// other form is its own name, so it reads as another package than the app and the rebuild check
/// fails loudly instead of passing.
pub(crate) fn package_name(id: &str) -> &str {
    match id.rsplit_once('#') {
        Some((_, fragment)) if fragment.contains('@') => {
            fragment.split_once('@').map_or(fragment, |(name, _)| name)
        }
        Some((source, _)) => {
            let path = source.split_once('?').map_or(source, |(path, _)| path);
            path.trim_end_matches('/')
                .rsplit('/')
                .next()
                .unwrap_or(path)
        }
        None => id,
    }
}
