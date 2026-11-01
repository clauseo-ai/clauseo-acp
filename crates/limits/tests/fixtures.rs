//! `fixtures_are_redacted`: the secret check (`fixtures/check.rs`) over `fixtures/`, and the
//! check's own test on a planted bad file of each kind in a temp folder.
//!
//! A test crate root resolves modules beside itself, so each submodule names its path. Each
//! carries `#[cfg(test)]` so clippy treats its helpers as test code.

#[cfg(test)]
#[path = "live/write.rs"]
mod write;

#[cfg(test)]
#[path = "fixtures/check.rs"]
mod check;

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    use crate::check::check_dir;
    use crate::write;

    /// A temp folder of the test's own, removed at the end.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Scratch {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let n = NEXT.fetch_add(1, Ordering::Relaxed);
            let name = format!("clauseo-limits-fixtures-{}-{n}", std::process::id());
            let root = std::env::temp_dir().join(name);
            drop(write::remove_dir_all(&root));
            write::create_dir_all(&root).unwrap();
            Scratch(root)
        }

        /// Writes `contents` to `rel` (`/`-separated) under the folder.
        fn file(&self, rel: &str, contents: &str) -> PathBuf {
            let path = rel
                .split('/')
                .fold(self.0.clone(), |path, part| path.join(part));
            write::create_dir_all(path.parent().unwrap()).unwrap();
            write::write(&path, contents).unwrap();
            path
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            drop(write::remove_dir_all(&self.0));
        }
    }

    #[test]
    fn fixtures_are_redacted() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures");
        let report = check_dir(&root);
        assert!(report.files >= 20, "read only {} files", report.files);
        assert!(
            report.findings.is_empty(),
            "the fixtures fail the secret check:\n{}",
            report.findings.join("\n")
        );
    }

    #[test]
    fn the_secret_check_fails_on_each_planted_fault_and_names_its_file() {
        // Built at run time, so no source file holds a token shape.
        let sk = format!("sk-{}", "a1_-".repeat(6));
        let eyj = format!("eyJ{}", "Zm9v".repeat(6));
        let faults = [
            (
                "email/one.json",
                r#"{"email":"someone@corp.test"}"#.to_owned(),
            ),
            ("sk/deep/one.json", format!(r#"{{"key":"{sk}"}}"#)),
            ("eyj/one.jsonl", format!("{{}}\n{{\"token\":\"{eyj}\"}}\n")),
            ("codex/auth.json", "{}".to_owned()),
            ("claude/.credentials.json", "{}".to_owned()),
            ("readme/README.md", format!("`@anthropic-ai/x` and {sk}")),
        ];
        for (rel, contents) in faults {
            let scratch = Scratch::new();
            scratch.file(
                "clean.json",
                r#"{"email":"test@example.com","id":"redacted-1"}"#,
            );
            let bad = scratch.file(rel, &contents);
            let report = check_dir(&scratch.0);
            assert_eq!(report.files, 2, "{rel}");
            assert_eq!(report.findings.len(), 1, "{rel}: {:?}", report.findings);
            let finding = &report.findings[0];
            assert!(
                finding.starts_with(&bad.display().to_string()),
                "{rel}: {finding}"
            );
            assert!(
                !finding.contains(&sk) && !finding.contains(&eyj),
                "{rel}: quotes the token"
            );
        }
    }

    #[test]
    fn the_secret_check_passes_short_token_starts_and_example_com() {
        let scratch = Scratch::new();
        let short = format!("sk-{}", "a".repeat(19));
        scratch.file(
            "ok.json",
            &format!(r#"{{"a":"{short}","b":"x@example.com"}}"#),
        );
        let report = check_dir(&scratch.0);
        assert_eq!(report.files, 1);
        assert!(report.findings.is_empty(), "{:?}", report.findings);
    }
}
