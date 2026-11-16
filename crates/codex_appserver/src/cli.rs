//! Where the Codex CLI's `codex.js` is in an npm install of codex-acp: the one layout rule both
//! the sign-in (`agents`) and the usage reader (`limits`) follow, so neither runs the adapter's
//! own script for a Codex CLI command. Paths only: each caller checks the files with its own rule.

use std::path::{Component, Path, PathBuf};

/// Where the Codex CLI's script sits inside a `node_modules` folder: `@openai/codex`'s
/// `package.json` names `bin/codex.js` as its `codex` bin.
const CODEX_JS: [&str; 4] = ["@openai", "codex", "bin", "codex.js"];

/// Where to look for `codex.js` in the install folder `install`, whose adapter script is
/// `adapter_script` (`<install>/node_modules/@agentclientprotocol/codex-acp/dist/index.js`), in
/// order: the adapter package's own `node_modules`, then the install folder's. The first that
/// is there is the CLI. Always inside the install, never beside Node, which may be the user's own.
pub fn codex_js_candidates(install: &Path, adapter_script: &Path) -> Vec<PathBuf> {
    let modules = install.join("node_modules");
    package_dir(&modules, adapter_script)
        .map(|package| package.join("node_modules"))
        .into_iter()
        .chain(std::iter::once(modules))
        .map(|dir| CODEX_JS.iter().fold(dir, |path, part| path.join(part)))
        .collect()
}

/// The adapter's package folder: the folder right under `modules` that holds `script`, with
/// its scope (`@agentclientprotocol/codex-acp`).
fn package_dir(modules: &Path, script: &Path) -> Option<PathBuf> {
    let mut parts = script.strip_prefix(modules).ok()?.components();
    let Some(Component::Normal(first)) = parts.next() else {
        return None;
    };
    let mut package = modules.join(first);
    if first.as_encoded_bytes().starts_with(b"@")
        && let Some(Component::Normal(name)) = parts.next()
    {
        package.push(name);
    }
    Some(package)
}

#[cfg(test)]
mod tests;
