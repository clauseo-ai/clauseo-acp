//! Which incremental setting the timed builds run with, and where that setting came from. Two
//! readings: `resolve` reads the environment and the config files, the way cargo does for a
//! `[profile.dev]` build. `warm_up_incremental` reads what cargo really ran, from the rustc lines
//! of the warm-up's `-v` output, and that one is the authority.

use toml::Table;

/// Whether incremental compilation is on, and the place the answer came from, as people read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Incremental {
    pub on: bool,
    pub source: String,
}

/// The source named when `CI` decides: cargo turns incremental off whenever `CI` is present, with
/// any value, unless `CARGO_INCREMENTAL=1` or `build.incremental = true` brings it back.
pub(crate) const CI_SOURCE: &str =
    "CI is set: cargo turns incremental off unless CARGO_INCREMENTAL=1 or build.incremental = true";

/// What `resolve` reads. A `None` input is absent. A file that is absent, does not parse, or has no
/// usable `incremental` value is skipped.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Sources<'a> {
    /// `CARGO_INCREMENTAL`: on only when it is exactly `1`.
    pub cargo_incremental: Option<&'a str>,
    /// `CARGO_BUILD_INCREMENTAL`: `true` or `false`.
    pub build_incremental: Option<&'a str>,
    /// Whether the `CI` variable is present. Cargo reads presence, not the value.
    pub ci_present: bool,
    /// `CARGO_PROFILE_DEV_INCREMENTAL`: `true` or `false`.
    pub profile_dev_incremental: Option<&'a str>,
    /// The text of the repo's `.cargo/config.toml`.
    pub config_toml: Option<&'a str>,
    /// The text of the root `Cargo.toml`.
    pub manifest: Option<&'a str>,
}

/// The static reading, first match wins: `CARGO_INCREMENTAL`, `CARGO_BUILD_INCREMENTAL`,
/// `[build] incremental` in `.cargo/config.toml`, then `CI` (off),
/// `CARGO_PROFILE_DEV_INCREMENTAL`, `[profile.dev] incremental` in `.cargo/config.toml`,
/// `[profile.dev] incremental` in `Cargo.toml`, then cargo's dev default, which is on. `source`
/// names the one that decided.
pub(crate) fn resolve(sources: &Sources<'_>) -> Incremental {
    if let Some(value) = sources.cargo_incremental {
        return decided(value == "1", format!("CARGO_INCREMENTAL={value}"));
    }
    if let Some(on) = env_bool(sources.build_incremental) {
        return decided(on, format!("CARGO_BUILD_INCREMENTAL={on}"));
    }
    if let Some(on) = file_bool(sources.config_toml, &["build", "incremental"]) {
        return decided(
            on,
            format!("[build] incremental = {on} in .cargo/config.toml"),
        );
    }
    if sources.ci_present {
        return decided(false, CI_SOURCE);
    }
    if let Some(on) = env_bool(sources.profile_dev_incremental) {
        return decided(on, format!("CARGO_PROFILE_DEV_INCREMENTAL={on}"));
    }
    if let Some(on) = file_bool(sources.config_toml, &["profile", "dev", "incremental"]) {
        return decided(
            on,
            format!("[profile.dev] incremental = {on} in .cargo/config.toml"),
        );
    }
    if let Some(on) = file_bool(sources.manifest, &["profile", "dev", "incremental"]) {
        return decided(
            on,
            format!("[profile.dev] incremental = {on} in Cargo.toml"),
        );
    }
    decided(true, "cargo default")
}

fn decided(on: bool, source: impl Into<String>) -> Incremental {
    Incremental {
        on,
        source: source.into(),
    }
}

/// `value` as a bool when it is exactly `true` or `false`; anything else is skipped.
fn env_bool(value: Option<&str>) -> Option<bool> {
    match value {
        Some("true") => Some(true),
        Some("false") => Some(false),
        _ => None,
    }
}

/// The bool at `path` in the TOML `text`, when `text` parses and holds one there.
fn file_bool(text: Option<&str>, path: &[&str]) -> Option<bool> {
    let table: Table = text?.parse().ok()?;
    bool_at(&table, path)
}

fn bool_at(table: &Table, path: &[&str]) -> Option<bool> {
    match path {
        [] => None,
        [key] => table.get(*key)?.as_bool(),
        [key, rest @ ..] => bool_at(table.get(*key)?.as_table()?, rest),
    }
}

/// What the warm-up's `-v` output says about the crate `package`, whose rustc lines carry
/// `--crate-name <package with - as _>`. `None` when there is no such line, which means cargo did
/// not rebuild it. `Some(true)` when every such line passes `-C incremental=<dir>`, quoted or not
/// (see `unquoted`), and `Some(false)` when one does not.
///
/// The lines are split on whitespace, so a quoted value with a space in it, such as
/// `-C 'incremental=/Users/x/My Projects/target/debug/incremental'`, arrives in pieces. Only the
/// first piece is read, which is why the value is matched by its `incremental=` prefix: an exact
/// match on the whole value would need the line split the way a shell splits it.
pub(crate) fn warm_up_incremental(output: &str, package: &str) -> Option<bool> {
    let crate_name = package.replace('-', "_");
    let mut rebuilt = false;
    let mut incremental = true;
    for line in output.lines() {
        let tokens: Vec<&str> = line.split_whitespace().collect();
        let named = tokens
            .windows(2)
            .any(|pair| pair[0] == "--crate-name" && unquoted(pair[1]) == crate_name);
        if !named {
            continue;
        }
        rebuilt = true;
        incremental &= tokens
            .windows(2)
            .any(|pair| pair[0] == "-C" && unquoted(pair[1]).starts_with("incremental="));
    }
    rebuilt.then_some(incremental)
}

/// `token` without one leading `'` or `"`. `cargo -v` prints each argument through
/// `shell_escape::escape` (cargo-util's `ProcessBuilder`, shell-escape 0.1.5), which quotes it
/// in one of two ways:
/// - Unix style, on Unix and on Windows when `MSYSTEM` is set (Git Bash, MSYS2): an argument with
///   any character outside `A-Za-z0-9-_=/,.+` goes in single quotes, so a Windows path
///   (`'incremental=D:\a\...'`) and a Unix path with a space, `@` or `~` both do.
/// - Windows style otherwise (cmd, PowerShell): an argument goes in double quotes only when it
///   holds a space, tab, newline or `"`.
///
/// The closing quote goes too when `token` ends with the same one. A value split at a space keeps
/// its closing quote on a later piece, so its first piece loses only the opening one.
fn unquoted(token: &str) -> &str {
    match token.strip_prefix(['\'', '"']) {
        Some(rest) => rest.strip_suffix(&token[..1]).unwrap_or(rest),
        None => token,
    }
}

#[cfg(test)]
mod tests;
