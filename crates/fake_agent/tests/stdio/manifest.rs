//! `manifest_has_only_allowed_dependencies`: the crate's own Cargo.toml names only the four
//! allowed dependencies, and turns on no feature of the SDK.

/// The dependencies the crate may have. Synthesis §3.2 and the S6 brief.
const ALLOWED: &[&str] = &["agent-client-protocol", "serde", "serde_json", "thiserror"];

const MANIFEST: &str = include_str!("../../Cargo.toml");

/// Every problem in manifest `text`, one line each naming the dependency or the line.
fn problems(text: &str) -> Vec<String> {
    let mut problems = Vec::new();
    // The dependency a `[dependencies.<name>]` table describes, or "" in a plain table.
    let mut table_dependency = String::new();
    let mut in_dependencies: Option<&str> = None;
    for raw in text.lines() {
        let line = raw.split('#').next().unwrap_or_default().trim();
        if line.is_empty() {
            continue;
        }
        if let Some(header) = line.strip_prefix('[') {
            let header = header.trim_matches(|c| c == '[' || c == ']').trim();
            let (table, dependency) = split_table(header);
            in_dependencies = table;
            table_dependency = dependency.unwrap_or_default();
            if let Some(table) = table
                && !table_dependency.is_empty()
            {
                check_name(&table_dependency, table, &mut problems);
            }
            continue;
        }
        let Some(kind) = in_dependencies else {
            // `[features]` could still turn on `agent-client-protocol/<feature>`.
            if line.contains("agent-client-protocol/") || line.contains("agent-client-protocol?/") {
                problems.push(format!("turns on an agent-client-protocol feature: {line}"));
            }
            continue;
        };
        let key = line
            .split(['=', '.'])
            .next()
            .unwrap_or_default()
            .trim()
            .trim_matches('"');
        let name = if table_dependency.is_empty() {
            key.to_owned()
        } else {
            table_dependency.clone()
        };
        if table_dependency.is_empty() {
            check_name(&name, kind, &mut problems);
        }
        for banned in ["package", "path", "git"] {
            if has_key(line, banned) {
                problems.push(format!("{name}: `{banned}` is not allowed: {line}"));
            }
        }
        if name == "agent-client-protocol" && has_key(line, "features") {
            problems.push(format!("agent-client-protocol turns on a feature: {line}"));
        }
        if name == "agent-client-protocol"
            && line.contains("default-features")
            && !line.contains("default-features = false")
        {
            problems.push(format!(
                "agent-client-protocol turns on its default features: {line}"
            ));
        }
    }
    problems
}

/// For a table header, `Some(kind)` when it is a dependency table, and the dependency it names
/// in the `[dependencies.<name>]` form.
fn split_table(header: &str) -> (Option<&'static str>, Option<String>) {
    let parts: Vec<&str> = header
        .split('.')
        .map(|p| p.trim().trim_matches('"'))
        .collect();
    for kind in ["dependencies", "dev-dependencies", "build-dependencies"] {
        if let Some(index) = parts.iter().position(|p| *p == kind) {
            let leading = &parts[..index];
            // `[dependencies]`, or `[target.'cfg(..)'.dependencies]`.
            if leading.is_empty() || leading[0] == "target" {
                return (Some(kind), parts.get(index + 1).map(|p| (*p).to_owned()));
            }
        }
    }
    (None, None)
}

fn check_name(name: &str, table: &str, problems: &mut Vec<String>) {
    if table != "dependencies" {
        problems.push(format!("{name}: no {table} allowed"));
    } else if !ALLOWED.contains(&name) {
        problems.push(format!(
            "{name}: not an allowed dependency (allowed: {})",
            ALLOWED.join(", ")
        ));
    }
}

/// Whether `line` sets key `key`, inline (`{ key = ...`) or as its own line (`key = ...`).
fn has_key(line: &str, key: &str) -> bool {
    line.split(['{', ',', '.']).any(|part| {
        part.trim()
            .strip_prefix(key)
            .is_some_and(|rest| rest.trim_start().starts_with('='))
    })
}

#[test]
fn manifest_has_only_allowed_dependencies() {
    assert_eq!(problems(MANIFEST), Vec::<String>::new());
    // The planted violations of checks 7 and 8 of the brief, and the ways around them.
    let with = |extra: &str| problems(&format!("{MANIFEST}\n{extra}\n"));
    let protocol = with("[dependencies.clauseo-protocol]\npath = \"../protocol\"");
    assert!(
        protocol.iter().any(|p| p.contains("clauseo-protocol")),
        "{protocol:?}"
    );
    let planted = MANIFEST.replace(
        "thiserror = { workspace = true }",
        "thiserror = { workspace = true }\nclauseo-protocol = { path = \"../protocol\" }",
    );
    let protocol = problems(&planted);
    assert!(
        protocol
            .iter()
            .any(|p| p.starts_with("clauseo-protocol: not an allowed")),
        "{protocol:?}"
    );
    let features = problems(&MANIFEST.replace(
        "agent-client-protocol = { workspace = true }",
        "agent-client-protocol = { workspace = true, features = [\"unstable\"] }",
    ));
    assert!(
        features
            .iter()
            .any(|p| p.contains("features = [\"unstable\"]")),
        "{features:?}"
    );
    let dotted =
        with("[dependencies.agent-client-protocol]\nworkspace = true\nfeatures = [\"unstable\"]");
    assert!(
        dotted.iter().any(|p| p.contains("turns on a feature")),
        "{dotted:?}"
    );
    let via_features = with("[features]\ndefault = [\"agent-client-protocol/unstable\"]");
    assert!(
        via_features
            .iter()
            .any(|p| p.contains("agent-client-protocol/unstable")),
        "{via_features:?}"
    );
    let dev = with("[dev-dependencies]\ntempfile = \"3\"");
    assert!(
        dev.iter()
            .any(|p| p.starts_with("tempfile: no dev-dependencies")),
        "{dev:?}"
    );
    let build = with("[target.'cfg(unix)'.build-dependencies]\ncc = \"1\"");
    assert!(
        build
            .iter()
            .any(|p| p.starts_with("cc: no build-dependencies")),
        "{build:?}"
    );
    let renamed = MANIFEST.replace(
        "serde_json = { workspace = true }",
        "serde_json = { package = \"clauseo-protocol\", path = \"../protocol\" }",
    );
    let renamed = problems(&renamed);
    assert!(
        renamed
            .iter()
            .any(|p| p.contains("`package` is not allowed")),
        "{renamed:?}"
    );
    let defaults = with("[dependencies.agent-client-protocol]\ndefault-features = true");
    assert!(
        defaults.iter().any(|p| p.contains("default features")),
        "{defaults:?}"
    );
}
