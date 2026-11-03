//! The `agent_browser_pin` test: `crates/desktop/agent-browser.toml`, the agent-browser sidecar a
//! release build bundles (ADR 0029, "Bundling (desktop)"), is well formed. Its header says what
//! the format is; `sidecars/agent-browser.mjs` reads the same file, and its tests run under
//! node, not here.

use std::collections::BTreeSet;

/// The pin, read at build time; it sits next to this crate's manifest.
const PIN: &str = include_str!("../agent-browser.toml");

/// The download folder every pinned url starts with, before `v<version>/`.
const RELEASE: &str = "https://github.com/vercel-labs/agent-browser/releases/download/";

/// Each Rust target triple the pin names, and the release asset it must download. Windows ARM64
/// takes the x64 build (upstream ships no ARM64 build).
const ASSETS: [(&str, &str); 6] = [
    ("aarch64-apple-darwin", "agent-browser-darwin-arm64"),
    ("x86_64-apple-darwin", "agent-browser-darwin-x64"),
    ("x86_64-unknown-linux-gnu", "agent-browser-linux-x64"),
    ("aarch64-unknown-linux-gnu", "agent-browser-linux-arm64"),
    ("x86_64-pc-windows-msvc", "agent-browser-win32-x64.exe"),
    ("aarch64-pc-windows-msvc", "agent-browser-win32-x64.exe"),
];

#[test]
fn agent_browser_pin() {
    let pin: toml::Table = toml::from_str(PIN).expect("agent-browser.toml is TOML");

    let version = pin
        .get("version")
        .and_then(toml::Value::as_str)
        .expect("agent-browser.toml has version = \"...\"");
    semver::Version::parse(version).expect("the pinned version is semver");

    let mut triples: BTreeSet<&str> = pin.keys().map(String::as_str).collect();
    triples.remove("version");
    let expected: BTreeSet<&str> = ASSETS.iter().map(|(triple, _)| *triple).collect();
    assert_eq!(triples, expected, "the pin names exactly the six triples");

    for (triple, asset) in ASSETS {
        let table = pin[triple]
            .as_table()
            .unwrap_or_else(|| panic!("[{triple}] is a table"));
        if table.get("none").and_then(toml::Value::as_bool) == Some(true) {
            assert_eq!(
                table.len(),
                1,
                "[{triple}] with none = true holds nothing else"
            );
            continue;
        }
        assert_eq!(table.len(), 2, "[{triple}] holds url and sha256 only");

        let url = table
            .get("url")
            .and_then(toml::Value::as_str)
            .unwrap_or_else(|| panic!("[{triple}] has a url"));
        assert_eq!(
            url,
            format!("{RELEASE}v{version}/{asset}"),
            "[{triple}] downloads its asset from the pinned release"
        );

        let sha = table
            .get("sha256")
            .and_then(toml::Value::as_str)
            .unwrap_or_else(|| panic!("[{triple}] has a sha256"));
        assert!(
            sha.len() == 64
                && sha
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "[{triple}] sha256 is 64 lowercase hex characters: {sha}"
        );
    }
}
