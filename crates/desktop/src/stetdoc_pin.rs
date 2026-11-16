//! The `stetdoc_pin` test: `crates/desktop/stetdoc.toml`, the stetdoc a release build bundles in
//! its own folder (the private task row U13), is well formed and has a table for every target
//! release.yml builds, each pinned (url and sha256); a `pending` table is refused by the release
//! fetch, and none is checked in. Its header says what
//! the format is; `sidecars/stetdoc.mjs` reads the same file, and its tests run under node.

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    /// The pin, read at build time; it sits next to this crate's manifest.
    const PIN: &str = include_str!("../stetdoc.toml");

    /// The release script, whose `RELEASE_TARGETS` is release.yml's build matrix.
    const SIDECARS: &str = include_str!("../../../.github/scripts/sidecars.mjs");

    /// The download folder every pinned url starts with, before `v<version>/`.
    const RELEASE: &str = "https://github.com/clauseo-ai/stetdoc/releases/download/";

    /// `RELEASE_TARGETS` from `sidecars.mjs`: the quoted strings of its array.
    fn release_targets() -> BTreeSet<String> {
        let (_, rest) = SIDECARS
            .split_once("export const RELEASE_TARGETS = [")
            .expect("sidecars.mjs exports RELEASE_TARGETS");
        let (list, _) = rest.split_once("];").expect("RELEASE_TARGETS ends with ];");
        let targets: BTreeSet<String> = list
            .split(',')
            .map(|item| item.trim().trim_matches('"').to_owned())
            .filter(|item| !item.is_empty())
            .collect();
        assert!(targets.len() >= 3, "RELEASE_TARGETS read as {targets:?}");
        targets
    }

    /// The asset a triple's url must name: `.zip` on Windows, `.tar.gz` elsewhere.
    fn asset(triple: &str) -> String {
        let ext = if triple.contains("-windows-") {
            "zip"
        } else {
            "tar.gz"
        };
        format!("stetdoc-{triple}.{ext}")
    }

    /// Everything wrong with `text` as the pin for `targets`, one message each. Empty when it is
    /// well formed.
    fn problems(text: &str, targets: &BTreeSet<String>) -> Vec<String> {
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

        let triples: BTreeSet<String> = table
            .keys()
            .filter(|key| *key != "version")
            .cloned()
            .collect();
        if &triples != targets {
            out.push(format!(
                "the tables are {triples:?}, expected the release targets {targets:?}"
            ));
        }

        for triple in targets {
            let Some(entry) = table.get(triple).and_then(toml::Value::as_table) else {
                continue; // reported above
            };
            let keys: BTreeSet<&str> = entry.keys().map(String::as_str).collect();
            if keys == BTreeSet::from(["pending"]) {
                let why = entry
                    .get("pending")
                    .and_then(toml::Value::as_str)
                    .unwrap_or_default();
                if !why.starts_with("PENDING_") {
                    out.push(format!(
                        "[{triple}] pending must start with PENDING_: {why:?}"
                    ));
                }
                continue;
            }
            if keys != BTreeSet::from(["sha256", "url"]) {
                out.push(format!(
                    "[{triple}] holds {keys:?}, expected url and sha256, or pending"
                ));
            }
            let url = entry
                .get("url")
                .and_then(toml::Value::as_str)
                .unwrap_or_default();
            let want = format!("{RELEASE}v{version}/{}", asset(triple));
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
        out
    }

    /// The pin with the x64 Windows table's url and sha256 replaced by `body`.
    fn with_windows_x64(body: &str) -> String {
        let (head, rest) = PIN
            .split_once("[x86_64-pc-windows-msvc]\n")
            .expect("the Windows table is in the pin");
        let (_, tail) = rest
            .split_once("\n[")
            .expect("another table follows the Windows one");
        format!("{head}[x86_64-pc-windows-msvc]\n{body}\n[{tail}")
    }

    #[test]
    fn stetdoc_pin_has_every_release_target_pinned() {
        let problems = problems(PIN, &release_targets());
        assert!(problems.is_empty(), "stetdoc.toml: {problems:#?}");
        assert!(
            !PIN.lines().any(|line| line.starts_with("pending")),
            "stetdoc.toml still holds a pending table"
        );
    }

    #[test]
    fn the_release_targets_are_the_five_release_yml_builds() {
        let want: BTreeSet<String> = [
            "aarch64-apple-darwin",
            "x86_64-apple-darwin",
            "x86_64-pc-windows-msvc",
            "aarch64-pc-windows-msvc",
            "x86_64-unknown-linux-gnu",
        ]
        .map(str::to_owned)
        .into();
        assert_eq!(release_targets(), want);
    }

    #[test]
    fn a_missing_target_is_refused() {
        let (head, _) = PIN
            .split_once("[x86_64-unknown-linux-gnu]")
            .expect("the Linux table is in the pin");
        let (_, tail) = PIN
            .split_once("[x86_64-pc-windows-msvc]")
            .expect("the Windows table is in the pin");
        let text = format!("{head}[x86_64-pc-windows-msvc]{tail}");
        let problems = problems(&text, &release_targets());
        assert!(
            problems.iter().any(|p| p.contains("the tables are")),
            "{problems:#?}"
        );
    }

    #[test]
    fn a_pending_entry_must_say_pending_and_may_not_carry_a_url() {
        let named = with_windows_x64("pending = \"PENDING_TEST: not yet\"\n");
        let problems_named = problems(&named, &release_targets());
        assert!(problems_named.is_empty(), "{problems_named:#?}");
        let unnamed = with_windows_x64("pending = \"later\"\n");
        let problems_unnamed = problems(&unnamed, &release_targets());
        assert!(
            problems_unnamed
                .iter()
                .any(|p| p.contains("must start with PENDING_")),
            "{problems_unnamed:#?}"
        );
        let mixed =
            with_windows_x64("pending = \"PENDING_TEST\"\nurl = \"https://example.com/x\"\n");
        let problems_mixed = problems(&mixed, &release_targets());
        assert!(
            problems_mixed
                .iter()
                .any(|p| p.contains("expected url and sha256, or pending")),
            "{problems_mixed:#?}"
        );
    }

    #[test]
    fn a_url_for_another_triple_or_version_is_refused() {
        let text = PIN.replacen(
            "/stetdoc-aarch64-apple-darwin.tar.gz",
            "/stetdoc-x86_64-apple-darwin.tar.gz",
            1,
        );
        assert_ne!(text, PIN, "the arm64 macOS url is in the pin");
        let problems = problems(&text, &release_targets());
        assert!(
            problems
                .iter()
                .any(|p| p.contains("[aarch64-apple-darwin] url is")),
            "{problems:#?}"
        );
    }
}
