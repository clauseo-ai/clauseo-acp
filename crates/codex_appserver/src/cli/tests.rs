//! The codex.js candidates: the adapter package's own `node_modules` first, then the install's.

use std::path::{Path, PathBuf};

use super::codex_js_candidates;

fn under(root: &Path, parts: &str) -> PathBuf {
    parts
        .split('/')
        .fold(root.to_path_buf(), |path, part| path.join(part))
}

#[test]
fn codex_js_is_looked_for_under_the_adapter_then_the_install() {
    let install = Path::new("install");
    let script = under(
        install,
        "node_modules/@agentclientprotocol/codex-acp/dist/index.js",
    );
    assert_eq!(
        codex_js_candidates(install, &script),
        vec![
            under(
                install,
                "node_modules/@agentclientprotocol/codex-acp/node_modules/@openai/codex/bin/codex.js",
            ),
            under(install, "node_modules/@openai/codex/bin/codex.js"),
        ]
    );
}

#[test]
fn a_script_outside_the_install_looks_only_in_the_install() {
    let install = Path::new("install");
    assert_eq!(
        codex_js_candidates(install, Path::new("elsewhere/index.js")),
        vec![under(install, "node_modules/@openai/codex/bin/codex.js")]
    );
}
