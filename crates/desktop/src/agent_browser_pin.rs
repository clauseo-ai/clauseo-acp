//! The `agent_browser_pin` test: `crates/desktop/agent-browser.toml`, the agent-browser sidecar a
//! release build bundles (ADR 0029, "Bundling (desktop)"), is well formed. Its header says what
//! the format is; `sidecars/agent-browser.mjs` reads the same file, and its tests run under
//! node, not here. macOS and Windows take our build of upstream plus our patches
//! (`crates/desktop/agent-browser/README.md`), from our release under `patched_tag`; Linux keeps
//! upstream's release binary for now (development and CI only).

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    /// The pin, read at build time; it sits next to this crate's manifest.
    const PIN: &str = include_str!("../agent-browser.toml");

    /// Upstream's download folder, before `v<version>/`.
    const UPSTREAM: &str = "https://github.com/vercel-labs/agent-browser/releases/download/";

    /// Our download folder, before `<patched_tag>/`.
    const OURS: &str = "https://github.com/clauseo-ai/clauseo-acp/releases/download/";

    /// The keys before the first table.
    const TOP_KEYS: [&str; 5] = [
        "version",
        "patched_tag",
        "upstream_commit",
        "upstream_tarball_sha256",
        "rustc_commit",
    ];

    /// Each Rust target triple the pin names, and upstream's release asset it must download, or
    /// `None` for a triple that takes our build (`agent-browser-<triple>`, `.exe` on Windows,
    /// under `patched_tag`). Each of ours is its own build: Windows ARM64 is native, not the x64
    /// build under emulation.
    const ASSETS: [(&str, Option<&str>); 6] = [
        ("aarch64-apple-darwin", None),
        ("x86_64-apple-darwin", None),
        ("x86_64-unknown-linux-gnu", Some("agent-browser-linux-x64")),
        (
            "aarch64-unknown-linux-gnu",
            Some("agent-browser-linux-arm64"),
        ),
        ("x86_64-pc-windows-msvc", None),
        ("aarch64-pc-windows-msvc", None),
    ];

    /// The sha256 a table of ours holds before the build workflow has run. None of our tables may
    /// hold it (all zeros is what `is_hex` would otherwise accept).
    const PLACEHOLDER: &str = "0000000000000000000000000000000000000000000000000000000000000000";

    /// Whether `text` is `len` lowercase hex characters.
    fn is_hex(text: &str, len: usize) -> bool {
        text.len() == len
            && text
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    }

    /// Whether `tag` is `agent-browser-v<version>-clauseo.<n>`, n a number from 1 up.
    fn is_patched_tag(tag: &str, version: &str) -> bool {
        let Some(n) = tag
            .strip_prefix("agent-browser-v")
            .and_then(|rest| rest.strip_prefix(version))
            .and_then(|rest| rest.strip_prefix("-clauseo."))
        else {
            return false;
        };
        !n.is_empty() && !n.starts_with('0') && n.bytes().all(|b| b.is_ascii_digit())
    }

    /// Everything wrong with `text` as the pin, one message each. Empty when it is well formed.
    /// Today no triple may hold `none = true`: a triple with no build is a deliberate change to
    /// this test, not a pin edit.
    fn problems(text: &str) -> Vec<String> {
        let mut out = Vec::new();
        let table: toml::Table = match toml::from_str(text) {
            Ok(table) => table,
            Err(err) => return vec![format!("not TOML: {err}")],
        };
        let top = |key: &str| table.get(key).and_then(toml::Value::as_str);
        let Some(version) = top("version") else {
            return vec!["no version = \"...\"".to_owned()];
        };
        if semver::Version::parse(version).is_err() {
            out.push(format!("version {version} is not semver"));
        }
        let patched_tag = top("patched_tag").unwrap_or_default();
        if !is_patched_tag(patched_tag, version) {
            out.push(format!(
                "patched_tag {patched_tag:?} is not agent-browser-v{version}-clauseo.<n>"
            ));
        }
        if !is_hex(top("upstream_commit").unwrap_or_default(), 40) {
            out.push("upstream_commit is not 40 lowercase hex characters".to_owned());
        }
        if !is_hex(top("upstream_tarball_sha256").unwrap_or_default(), 64) {
            out.push("upstream_tarball_sha256 is not 64 lowercase hex characters".to_owned());
        }
        if !is_hex(top("rustc_commit").unwrap_or_default(), 40) {
            out.push("rustc_commit is not 40 lowercase hex characters".to_owned());
        }

        let triples: BTreeSet<&str> = table
            .keys()
            .map(String::as_str)
            .filter(|key| !TOP_KEYS.contains(key))
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
            let want = match asset {
                Some(asset) => format!("{UPSTREAM}v{version}/{asset}"),
                None if triple.ends_with("-pc-windows-msvc") => {
                    format!("{OURS}{patched_tag}/agent-browser-{triple}.exe")
                }
                None => format!("{OURS}{patched_tag}/agent-browser-{triple}"),
            };
            if url != want {
                out.push(format!("[{triple}] url is {url}, expected {want}"));
            }
            let sha = entry
                .get("sha256")
                .and_then(toml::Value::as_str)
                .unwrap_or_default();
            if !is_hex(sha, 64) {
                out.push(format!(
                    "[{triple}] sha256 is not 64 lowercase hex characters: {sha}"
                ));
            }
            if asset.is_none() && sha == PLACEHOLDER {
                out.push(format!(
                    "[{triple}] sha256 is the all-zero placeholder: paste the sha256 the build workflow printed"
                ));
            }
        }

        // Our builds are distinct: no two of our tables share a url or a sha256.
        let ours: Vec<(&str, &str, &str)> = ASSETS
            .iter()
            .filter(|(_, asset)| asset.is_none())
            .filter_map(|(triple, _)| {
                let entry = table.get(*triple)?.as_table()?;
                let url = entry.get("url")?.as_str()?;
                Some((*triple, url, entry.get("sha256")?.as_str()?))
            })
            .collect();
        for (i, (first, url_a, sha_a)) in ours.iter().enumerate() {
            for (second, url_b, sha_b) in &ours[i + 1..] {
                if url_a == url_b {
                    out.push(format!("[{first}] and [{second}] must carry distinct urls"));
                }
                if sha_a == sha_b {
                    out.push(format!(
                        "[{first}] and [{second}] must carry distinct sha256s"
                    ));
                }
            }
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

    /// The old arrangement, both Windows triples on upstream's x64 asset, is refused.
    #[test]
    fn windows_on_upstreams_x64_asset_is_refused() {
        let ours = "https://github.com/clauseo-ai/clauseo-acp/releases/download/\
                    agent-browser-v0.41.1-clauseo.1/agent-browser-aarch64-pc-windows-msvc.exe";
        let upstream = "https://github.com/vercel-labs/agent-browser/releases/download/\
                        v0.41.1/agent-browser-win32-x64.exe";
        assert!(PIN.contains(ours), "the ARM64 table holds our url");
        let problems = problems(&PIN.replacen(ours, upstream, 1));
        assert!(
            problems
                .iter()
                .any(|p| p.starts_with("[aarch64-pc-windows-msvc] url is")),
            "{problems:#?}"
        );
    }

    /// macOS takes our build today; upstream's binary there is refused.
    #[test]
    fn macos_on_upstreams_asset_is_refused() {
        let ours = "https://github.com/clauseo-ai/clauseo-acp/releases/download/\
                    agent-browser-v0.41.1-clauseo.1/agent-browser-aarch64-apple-darwin\"";
        let upstream = "https://github.com/vercel-labs/agent-browser/releases/download/\
                        v0.41.1/agent-browser-darwin-arm64\"";
        assert!(PIN.contains(ours), "the ARM64 macOS table holds our url");
        let found = problems(&PIN.replacen(ours, upstream, 1));
        assert!(
            found
                .iter()
                .any(|p| p.starts_with("[aarch64-apple-darwin] url is")),
            "{found:#?}"
        );
    }

    #[test]
    fn our_builds_must_not_share_one_build() {
        let x64 = "agent-browser-x86_64-pc-windows-msvc.exe";
        let arm64 = "agent-browser-aarch64-pc-windows-msvc.exe";
        let found = problems(&PIN.replacen(arm64, x64, 1));
        assert!(
            found.iter().any(|p| p.contains("must carry distinct urls")),
            "{found:#?}"
        );

        // Copy the x64 table's pinned sha256 into the ARM64 table: a real hash, not a placeholder.
        let (x64_sha, arm64_sha) = (
            pinned_sha("x86_64-pc-windows-msvc"),
            pinned_sha("aarch64-pc-windows-msvc"),
        );
        assert_ne!(
            x64_sha, arm64_sha,
            "the two Windows tables hold distinct hashes"
        );
        let from = format!("sha256 = \"{arm64_sha}\"");
        let to = format!("sha256 = \"{x64_sha}\"");
        let found = problems(&PIN.replacen(&from, &to, 1));
        assert!(
            found
                .iter()
                .any(|p| p.contains("must carry distinct sha256s")),
            "{found:#?}"
        );
    }

    /// Our four tables, the ones that take our build (no upstream asset).
    const OURS_TRIPLES: [&str; 4] = [
        "x86_64-pc-windows-msvc",
        "aarch64-pc-windows-msvc",
        "aarch64-apple-darwin",
        "x86_64-apple-darwin",
    ];

    /// The sha256 the pin holds for `triple`.
    fn pinned_sha(triple: &str) -> String {
        let table: toml::Table = toml::from_str(PIN).expect("the pin parses");
        table[triple].as_table().expect("a table")["sha256"]
            .as_str()
            .expect("a sha256")
            .to_owned()
    }

    /// An all-zero sha256 (the placeholder) is refused in each of our four tables, and nothing else
    /// about the pin changes: the message names the triple.
    #[test]
    fn a_placeholder_is_refused_in_each_of_our_tables() {
        for triple in OURS_TRIPLES {
            let real = pinned_sha(triple);
            let from = format!("sha256 = \"{real}\"");
            let to = format!("sha256 = \"{PLACEHOLDER}\"");
            let text = PIN.replacen(&from, &to, 1);
            assert_ne!(text, PIN, "[{triple}] the test replaced its sha256");
            let found = problems(&text);
            let want = format!("[{triple}] sha256 is the all-zero placeholder");
            assert!(found.iter().any(|p| p.starts_with(&want)), "{found:#?}");
            assert_eq!(
                found.len(),
                1,
                "only the placeholder is reported: {found:#?}"
            );
        }
    }

    /// An all-zero sha256 is not an allowance for upstream's tables: they keep the hex check only.
    #[test]
    fn a_zero_sha_on_upstreams_linux_table_is_not_this_check() {
        let real = pinned_sha("x86_64-unknown-linux-gnu");
        let from = format!("sha256 = \"{real}\"");
        let to = format!("sha256 = \"{PLACEHOLDER}\"");
        let found = problems(&PIN.replacen(&from, &to, 1));
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn the_top_keys_are_checked() {
        let cases = [
            ("-clauseo.1\"", "-clauseo.0\""),
            (
                "patched_tag = \"agent-browser-v0.41.1",
                "patched_tag = \"agent-browser-v0.41.2",
            ),
            ("upstream_commit = \"5", "upstream_commit = \""),
            (
                "upstream_tarball_sha256 = \"c",
                "upstream_tarball_sha256 = \"C",
            ),
            ("rustc_commit = \"b", "rustc_commit = \"B"),
        ];
        for (from, to) in cases {
            assert!(PIN.contains(from), "the pin holds {from}");
            let problems = problems(&PIN.replacen(from, to, 1));
            assert!(!problems.is_empty(), "{from} -> {to} passed");
        }
        let extra = PIN.replacen("version = ", "upstream = \"x\"\nversion = ", 1);
        assert!(
            problems(&extra)
                .iter()
                .any(|p| p.starts_with("the tables are")),
            "an unknown top key passed"
        );
    }
}
