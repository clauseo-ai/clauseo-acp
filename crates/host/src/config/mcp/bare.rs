//! Whether an imported program is stored by its bare file name or its absolute path.
//!
//! A bare name re-resolves at each launch (A1's `resolve_command`: the PATH, then the folders
//! each OS's tools install into), so it survives an upgrade that moves the program, and it also
//! follows a later PATH change or a program put earlier on the PATH. The name is kept only where
//! upgrades do move programs: under a version manager's or Homebrew's folder. Anything else (a
//! user folder such as `~/bin` or `~/.local/bin`, a system folder) keeps its absolute path, the
//! safe side.

use std::path::{Path, PathBuf};

use clauseo_settings::mcp_import::{Env, Os, resolve_command};

/// Turns a path into its canonical form; `None` when it cannot (missing, unreadable).
pub(crate) type Canonical<'a> = &'a dyn Fn(&Path) -> Option<PathBuf>;

/// The bare file name to store for `command` (absolute, as the finder resolved it), or `None`
/// to keep the absolute path. The name is kept when searching for it under `env` finds the same
/// program and that program lives under a version-manager root ([`roots`]), both compared
/// canonically; any canonical form that cannot be made keeps the absolute path.
pub(crate) fn bare_name(env: &Env<'_>, canonical: Canonical<'_>, command: &Path) -> Option<String> {
    let name = command.file_name()?.to_str()?;
    let program = canonical(command)?;
    let found = canonical(&resolve_command(env, name)?)?;
    if found != program {
        return None;
    }
    let managed = roots(env)
        .iter()
        .filter_map(|root| canonical(root))
        .any(|root| program.starts_with(root));
    managed.then(|| name.to_owned())
}

/// The folders an upgrade moves programs within: nvm, Volta, asdf, mise and Homebrew on a Mac;
/// nvm-windows and Volta on Windows. Each tool's own variable comes first when set.
fn roots(env: &Env<'_>) -> Vec<PathBuf> {
    let var = |name: &str| (env.vars)(name).filter(|value| !value.is_empty());
    let home = |rel: &str| env.home.join(rel);
    let mut roots: Vec<PathBuf> = match env.os {
        Os::Mac => ["NVM_DIR", "VOLTA_HOME", "ASDF_DATA_DIR", "MISE_DATA_DIR"]
            .into_iter()
            .filter_map(var)
            .map(PathBuf::from)
            .chain([
                home(".nvm"),
                home(".volta"),
                home(".asdf"),
                home(".local/share/mise"),
                PathBuf::from("/opt/homebrew"),
                PathBuf::from("/usr/local/Cellar"),
                PathBuf::from("/usr/local/Homebrew"),
            ])
            .collect(),
        Os::Windows => ["NVM_HOME", "NVM_SYMLINK", "VOLTA_HOME"]
            .into_iter()
            .filter_map(var)
            .map(PathBuf::from)
            .chain(var("APPDATA").map(|dir| Path::new(&dir).join("nvm")))
            .chain(var("LOCALAPPDATA").map(|dir| Path::new(&dir).join("Volta")))
            .collect(),
    };
    roots.retain(|root| root.is_absolute());
    roots
}

#[cfg(all(test, unix))]
#[path = "bare_tests.rs"]
mod tests;
