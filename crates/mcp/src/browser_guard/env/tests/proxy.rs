//! The proxy is optional: the plain client's Codex entry sends none.

use std::path::Path;

use super::super::{Os, WindowsDirs, build};
use super::{abs, get, source_for};

/// No `AGENT_BROWSER_PROXY` (the plain client's Codex entry): the guard starts, and the child
/// gets no proxy variable of any spelling and no bypass; every other variable is as with one. A
/// proxy that is given must still be valid (`each_required_variable_invalid_is_named`).
#[test]
fn a_missing_proxy_sets_no_proxy_variable() {
    for os in [Os::Unix, Os::Windows] {
        let cwd = abs("cwd");
        let with = build(
            &source_for(os, &[], ""),
            os,
            Path::new(&cwd),
            &WindowsDirs::default(),
        )
        .unwrap();
        let without = build(
            &source_for(
                os,
                &[("HTTPS_PROXY", "http://evil:1")],
                "AGENT_BROWSER_PROXY",
            ),
            os,
            Path::new(&cwd),
            &WindowsDirs::default(),
        )
        .unwrap();
        assert!(
            without
                .keys()
                .all(|name| !name.to_ascii_uppercase().contains("PROXY")),
            "{os:?}: {without:?}"
        );
        let mut expected = with.clone();
        expected.retain(|name, _| !name.to_ascii_uppercase().contains("PROXY"));
        assert_eq!(without, expected, "{os:?}");
        assert_eq!(get(&with, "AGENT_BROWSER_PROXY"), "http://127.0.0.1:8080");
    }
}
