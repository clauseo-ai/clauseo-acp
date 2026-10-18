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

/// What `resolve` reads. A `None` input is absent. A file that is absent, does not parse, or has no
/// usable `incremental` value is skipped.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Sources<'a> {
    /// `CARGO_INCREMENTAL`: on only when it is exactly `1`.
    pub cargo_incremental: Option<&'a str>,
    /// `CARGO_BUILD_INCREMENTAL`: `true` or `false`.
    pub build_incremental: Option<&'a str>,
    /// `CARGO_PROFILE_DEV_INCREMENTAL`: `true` or `false`.
    pub profile_dev_incremental: Option<&'a str>,
    /// The text of the repo's `.cargo/config.toml`.
    pub config_toml: Option<&'a str>,
    /// The text of the root `Cargo.toml`.
    pub manifest: Option<&'a str>,
}

/// The static reading, first match wins: `CARGO_INCREMENTAL`, `CARGO_BUILD_INCREMENTAL`,
/// `[build] incremental` in `.cargo/config.toml`, `CARGO_PROFILE_DEV_INCREMENTAL`,
/// `[profile.dev] incremental` in `.cargo/config.toml`, `[profile.dev] incremental` in `Cargo.toml`,
/// then cargo's dev default, which is on. `source` names the one that decided.
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
/// not rebuild it. `Some(true)` when every such line passes `-C incremental=`, and `Some(false)`
/// when one does not.
pub(crate) fn warm_up_incremental(output: &str, package: &str) -> Option<bool> {
    let crate_name = package.replace('-', "_");
    let mut rebuilt = false;
    let mut incremental = true;
    for line in output.lines() {
        let tokens: Vec<&str> = line.split_whitespace().collect();
        let named = tokens
            .windows(2)
            .any(|pair| pair[0] == "--crate-name" && pair[1] == crate_name);
        if !named {
            continue;
        }
        rebuilt = true;
        incremental &= tokens
            .windows(2)
            .any(|pair| pair[0] == "-C" && pair[1].starts_with("incremental="));
    }
    rebuilt.then_some(incremental)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG_DEV_OFF: &str = "[profile.dev]\nincremental = false\n";
    const MANIFEST_ON: &str = "[profile.dev]\ndebug = 1\nincremental = true  # on\n";

    fn only(
        cargo: Option<&'static str>,
        build: Option<&'static str>,
        profile_env: Option<&'static str>,
        config: Option<&'static str>,
        manifest: Option<&'static str>,
    ) -> Incremental {
        resolve(&Sources {
            cargo_incremental: cargo,
            build_incremental: build,
            profile_dev_incremental: profile_env,
            config_toml: config,
            manifest,
        })
    }

    #[test]
    fn cargo_incremental_is_on_only_when_it_is_exactly_one() {
        let on = only(Some("1"), Some("false"), None, Some(CONFIG_DEV_OFF), None);
        assert_eq!(
            on,
            Incremental {
                on: true,
                source: "CARGO_INCREMENTAL=1".into()
            }
        );
        for value in ["0", "true", "yes", ""] {
            let off = resolve(&Sources {
                cargo_incremental: Some(value),
                manifest: Some(MANIFEST_ON),
                ..Sources::default()
            });
            assert!(!off.on, "CARGO_INCREMENTAL={value:?} must be off");
            assert_eq!(off.source, format!("CARGO_INCREMENTAL={value}"));
        }
    }

    #[test]
    fn build_incremental_comes_second_and_takes_true_or_false() {
        let off = only(None, Some("false"), None, None, Some(MANIFEST_ON));
        assert_eq!(
            off,
            Incremental {
                on: false,
                source: "CARGO_BUILD_INCREMENTAL=false".into()
            }
        );
        let on = only(None, Some("true"), None, Some(CONFIG_DEV_OFF), None);
        assert!(on.on);
        assert_eq!(on.source, "CARGO_BUILD_INCREMENTAL=true");
        let unknown = only(None, Some("maybe"), None, None, Some(CONFIG_DEV_OFF));
        assert!(!unknown.on);
        assert_eq!(
            unknown.source,
            "[profile.dev] incremental = false in Cargo.toml"
        );
    }

    #[test]
    fn build_table_in_config_comes_third() {
        let config = "[build]\nincremental = false\n";
        let off = only(None, None, Some("true"), Some(config), Some(MANIFEST_ON));
        assert!(!off.on);
        assert_eq!(
            off.source,
            "[build] incremental = false in .cargo/config.toml"
        );
    }

    #[test]
    fn profile_dev_env_comes_fourth_and_takes_true_or_false() {
        let off = only(None, None, Some("false"), None, Some(MANIFEST_ON));
        assert_eq!(
            off,
            Incremental {
                on: false,
                source: "CARGO_PROFILE_DEV_INCREMENTAL=false".into()
            }
        );
        let on = only(None, None, Some("true"), Some(CONFIG_DEV_OFF), None);
        assert!(on.on);
        assert_eq!(on.source, "CARGO_PROFILE_DEV_INCREMENTAL=true");
        let empty = only(None, None, Some(""), None, Some(CONFIG_DEV_OFF));
        assert!(!empty.on);
        assert_eq!(
            empty.source,
            "[profile.dev] incremental = false in Cargo.toml"
        );
    }

    #[test]
    fn profile_dev_in_config_comes_fifth() {
        let off = only(None, None, None, Some(CONFIG_DEV_OFF), Some(MANIFEST_ON));
        assert!(!off.on);
        assert_eq!(
            off.source,
            "[profile.dev] incremental = false in .cargo/config.toml"
        );
    }

    #[test]
    fn profile_dev_in_the_manifest_comes_sixth_and_the_default_is_on() {
        let manifest = only(None, None, None, None, Some(CONFIG_DEV_OFF));
        assert!(!manifest.on);
        assert_eq!(
            manifest.source,
            "[profile.dev] incremental = false in Cargo.toml"
        );
        let default = only(None, None, None, None, None);
        assert_eq!(
            default,
            Incremental {
                on: true,
                source: "cargo default".into()
            }
        );
    }

    /// The five sources in order, each left out (`None`) or set to `on`. The config file holds
    /// both its `[build]` and its `[profile.dev]` lines.
    fn chain(values: [Option<bool>; 5]) -> Incremental {
        let [build_env, build_cfg, dev_env, dev_cfg, manifest] = values;
        let mut sections = Vec::new();
        if let Some(on) = build_cfg {
            sections.push(format!("[build]\nincremental = {on}\n"));
        }
        if let Some(on) = dev_cfg {
            sections.push(format!("[profile.dev]\nincremental = {on}\n"));
        }
        let config = sections.concat();
        let build_env = build_env.map(|on| if on { "true" } else { "false" });
        let dev_env = dev_env.map(|on| if on { "true" } else { "false" });
        let manifest = manifest.map(|on| format!("[profile.dev]\nincremental = {on}\n"));
        resolve(&Sources {
            cargo_incremental: None,
            build_incremental: build_env,
            profile_dev_incremental: dev_env,
            config_toml: (!config.is_empty()).then_some(config.as_str()),
            manifest: manifest.as_deref(),
        })
    }

    #[test]
    fn the_first_set_source_decides_whatever_comes_after_it() {
        for index in 0..5 {
            for on in [true, false] {
                // Source `index` says `on`, every later source says the opposite, and the earlier
                // ones are left out.
                let mut values = [None; 5];
                values[index] = Some(on);
                for later in values.iter_mut().skip(index + 1) {
                    *later = Some(!on);
                }
                let decided = chain(values);
                let expected = [
                    format!("CARGO_BUILD_INCREMENTAL={on}"),
                    format!("[build] incremental = {on} in .cargo/config.toml"),
                    format!("CARGO_PROFILE_DEV_INCREMENTAL={on}"),
                    format!("[profile.dev] incremental = {on} in .cargo/config.toml"),
                    format!("[profile.dev] incremental = {on} in Cargo.toml"),
                ];
                assert_eq!(decided.on, on, "source {index} says {on}");
                assert_eq!(decided.source, expected[index]);
            }
        }
    }

    #[test]
    fn a_file_that_does_not_parse_or_lacks_the_key_is_skipped() {
        let broken = only(
            None,
            None,
            None,
            Some("[build\nincremental ="),
            Some(MANIFEST_ON),
        );
        assert_eq!(
            broken.source,
            "[profile.dev] incremental = true in Cargo.toml"
        );
        let wrong_type = only(
            None,
            None,
            None,
            Some("[build]\nincremental = \"no\"\n"),
            None,
        );
        assert_eq!(wrong_type.source, "cargo default");
    }

    #[test]
    fn the_repo_manifest_turns_incremental_on() {
        let manifest =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../Cargo.toml"));
        let manifest = manifest.unwrap_or_default();
        let resolved = resolve(&Sources {
            manifest: Some(&manifest),
            ..Sources::default()
        });
        assert!(resolved.on);
        assert_eq!(
            resolved.source,
            "[profile.dev] incremental = true in Cargo.toml"
        );
    }

    const RUSTC_DESKTOP: &str = "     Running `/rust/bin/rustc --crate-name clauseo_desktop --edition=2024 crates/desktop/src/lib.rs --error-format=json --crate-type lib -C embed-bitcode=no -C debuginfo=1 -C metadata=4a2f -C incremental=/ws/target/debug/incremental -C strip=debuginfo --out-dir /ws/target/debug/deps`";
    const RUSTC_PROTOCOL: &str = "     Running `/rust/bin/rustc --crate-name clauseo_protocol --edition=2024 crates/protocol/src/lib.rs --crate-type lib -C embed-bitcode=no -C metadata=9b1c --out-dir /ws/target/debug/deps`";

    #[test]
    fn a_rustc_line_with_incremental_says_the_crate_is_incremental() {
        let output = format!("   Checking clauseo-desktop v0.1.0\n{RUSTC_DESKTOP}\n");
        assert_eq!(warm_up_incremental(&output, "clauseo-desktop"), Some(true));
    }

    #[test]
    fn a_rustc_line_without_incremental_says_the_crate_is_not() {
        assert_eq!(
            warm_up_incremental(RUSTC_PROTOCOL, "clauseo-protocol"),
            Some(false)
        );
        let mixed = format!("{RUSTC_DESKTOP}\n{RUSTC_PROTOCOL}\n");
        assert_eq!(warm_up_incremental(&mixed, "clauseo-desktop"), Some(true));
        assert_eq!(
            warm_up_incremental(
                &format!("{RUSTC_DESKTOP}\n{RUSTC_PROTOCOL}"),
                "clauseo_desktop"
            ),
            Some(true)
        );
    }

    #[test]
    fn no_rustc_line_for_the_crate_means_it_was_not_rebuilt() {
        let output = "    Fresh clauseo-desktop v0.1.0\n";
        assert_eq!(warm_up_incremental(output, "clauseo-desktop"), None);
        assert_eq!(warm_up_incremental("", "clauseo-desktop"), None);
        // A crate whose name only starts the same is not a match.
        assert_eq!(warm_up_incremental(RUSTC_DESKTOP, "clauseo-desk"), None);
    }
}
