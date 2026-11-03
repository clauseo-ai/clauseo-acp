//! The `agent_browser_pin` test: `crates/desktop/agent-browser.toml`, the agent-browser sidecar a
//! release build bundles (ADR 0029, "Bundling (desktop)"), is well formed. Its header says what
//! the format is; `sidecars/agent-browser.mjs` reads the same file, and its tests run under
//! node, not here.

#[cfg(test)]
mod tests {
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

    /// The two Windows triples, which must carry one build between them.
    const WINDOWS: [&str; 2] = ["x86_64-pc-windows-msvc", "aarch64-pc-windows-msvc"];

    /// Everything wrong with `text` as the pin, one message each. Empty when it is well formed.
    /// Today no triple may hold `none = true`: a triple with no build is a deliberate change to
    /// this test, not a pin edit.
    fn problems(text: &str) -> Vec<String> {
        let mut out = Vec::new();
        let table: toml::Table = match toml::from_str(text) {
            Ok(table) => table,
            Err(err) => return vec![format!("not TOML: {err}")],
        };
        let Some(version) = table.get("version").and_then(toml::Value::as_str) else {
            return vec!["no version = \"...\"".to_owned()];
        };
        if semver::Version::parse(version).is_err() {
            out.push(format!("version {version} is not semver"));
        }

        let triples: BTreeSet<&str> = table
            .keys()
            .map(String::as_str)
            .filter(|key| *key != "version")
            .collect();
        let expected: BTreeSet<&str> = ASSETS.iter().map(|(triple, _)| *triple).collect();
        if triples != expected {
            out.push(format!("the tables are {triples:?}, expected {expected:?}"));
        }

        for (triple, asset) in ASSETS {
            let Some(entry) = table.get(triple).and_then(toml::Value::as_table) else {
                continue; // reported above
            };
            if entry.contains_key("none") {
                out.push(format!(
                    "[{triple}] holds none: no triple may lack a build yet"
                ));
                continue;
            }
            let keys: BTreeSet<&str> = entry.keys().map(String::as_str).collect();
            if keys != BTreeSet::from(["sha256", "url"]) {
                out.push(format!(
                    "[{triple}] holds {keys:?}, expected url and sha256"
                ));
            }
            let url = entry
                .get("url")
                .and_then(toml::Value::as_str)
                .unwrap_or_default();
            let want = format!("{RELEASE}v{version}/{asset}");
            if url != want {
                out.push(format!("[{triple}] url is {url}, expected {want}"));
            }
            let sha = entry
                .get("sha256")
                .and_then(toml::Value::as_str)
                .unwrap_or_default();
            if sha.len() != 64
                || !sha
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                out.push(format!(
                    "[{triple}] sha256 is not 64 lowercase hex characters: {sha}"
                ));
            }
        }

        let field = |triple: &str, key: &str| {
            table
                .get(triple)
                .and_then(toml::Value::as_table)
                .and_then(|entry| entry.get(key))
                .and_then(toml::Value::as_str)
        };
        let [first, second] = WINDOWS;
        if field(first, "url") != field(second, "url")
            || field(first, "sha256") != field(second, "sha256")
        {
            out.push(format!(
                "[{first}] and [{second}] must carry the same url and sha256"
            ));
        }
        out
    }

    #[test]
    fn agent_browser_pin() {
        let problems = problems(PIN);
        assert!(problems.is_empty(), "agent-browser.toml: {problems:#?}");
    }

    #[test]
    fn a_none_triple_is_refused_today() {
        let text = PIN.replacen(
            "[aarch64-apple-darwin]\n",
            "[aarch64-apple-darwin]\nnone = true\n",
            1,
        );
        let problems = problems(&text);
        assert!(
            problems
                .iter()
                .any(|p| p.contains("[aarch64-apple-darwin] holds none")),
            "{problems:#?}"
        );
    }

    #[test]
    fn the_two_windows_triples_must_share_one_sha256() {
        const X64_SHA: &str = "ddab45e8bae84e5bc6aa1b084bceca9b579aff2e8272474d7d26953cf10965ee";
        let (head, tail) = PIN
            .split_once("[aarch64-pc-windows-msvc]")
            .expect("the ARM64 Windows table is in the pin");
        assert!(
            tail.contains(X64_SHA),
            "the ARM64 table holds the x64 sha256"
        );
        let text = format!(
            "{head}[aarch64-pc-windows-msvc]{}",
            tail.replacen(X64_SHA, &"0".repeat(64), 1)
        );
        let problems = problems(&text);
        assert!(
            problems
                .iter()
                .any(|p| p.contains("must carry the same url and sha256")),
            "{problems:#?}"
        );
    }
}
