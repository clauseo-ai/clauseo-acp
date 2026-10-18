//! Tests for `profile.rs`: the static reading and its order, and the warm-up reader.

use super::*;

const CONFIG_DEV_OFF: &str = "[profile.dev]\nincremental = false\n";
const MANIFEST_ON: &str = "[profile.dev]\ndebug = 1\nincremental = true  # on\n";

/// The resolver over the five file and env inputs, with `CI` absent.
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
        ..Sources::default()
    })
}

fn ci(
    cargo: Option<&'static str>,
    build: Option<&'static str>,
    profile_env: Option<&'static str>,
    config: Option<&'static str>,
    manifest: Option<&'static str>,
) -> Incremental {
    resolve(&Sources {
        cargo_incremental: cargo,
        build_incremental: build,
        ci_present: true,
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
fn ci_present_turns_incremental_off_whatever_its_value() {
    // Cargo reads only whether CI is present, so CI=1, CI=true, CI=0, CI=false and CI= are one
    // input.
    for value in ["1", "true", "0", "false", ""] {
        let present = Some(value).is_some();
        let off = resolve(&Sources {
            ci_present: present,
            manifest: Some(MANIFEST_ON),
            ..Sources::default()
        });
        assert!(!off.on, "CI={value:?} must turn incremental off");
        assert_eq!(off.source, CI_SOURCE);
    }
}

#[test]
fn ci_is_beaten_only_by_cargo_incremental_1_or_build_true() {
    assert_eq!(
        ci(Some("1"), Some("false"), None, None, Some(CONFIG_DEV_OFF)),
        Incremental {
            on: true,
            source: "CARGO_INCREMENTAL=1".into()
        }
    );
    assert_eq!(
        ci(None, Some("true"), None, None, None),
        Incremental {
            on: true,
            source: "CARGO_BUILD_INCREMENTAL=true".into()
        }
    );
    assert_eq!(
        ci(
            None,
            None,
            None,
            Some("[build]\nincremental = true\n"),
            None
        ),
        Incremental {
            on: true,
            source: "[build] incremental = true in .cargo/config.toml".into()
        }
    );
}

#[test]
fn ci_is_not_beaten_by_the_profile_settings() {
    let env = ci(None, None, Some("true"), None, None);
    assert!(!env.on);
    assert_eq!(env.source, CI_SOURCE);
    let config = ci(
        None,
        None,
        None,
        Some("[profile.dev]\nincremental = true\n"),
        None,
    );
    assert!(!config.on);
    assert_eq!(config.source, CI_SOURCE);
    let manifest = ci(None, None, None, None, Some(MANIFEST_ON));
    assert!(!manifest.on);
    assert_eq!(manifest.source, CI_SOURCE);
}

#[test]
fn an_earlier_off_setting_still_wins_over_ci() {
    let off = ci(None, Some("false"), None, None, None);
    assert!(!off.on);
    assert_eq!(off.source, "CARGO_BUILD_INCREMENTAL=false");
}

#[test]
fn without_ci_the_profile_settings_decide_as_before() {
    let env = only(None, None, Some("false"), None, Some(MANIFEST_ON));
    assert_eq!(
        env,
        Incremental {
            on: false,
            source: "CARGO_PROFILE_DEV_INCREMENTAL=false".into()
        }
    );
    let config = only(None, None, None, Some(CONFIG_DEV_OFF), Some(MANIFEST_ON));
    assert_eq!(
        config.source,
        "[profile.dev] incremental = false in .cargo/config.toml"
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

/// The five file and env sources in order, each left out (`None`) or set to `on`. The config file
/// holds both its `[build]` and its `[profile.dev]` lines.
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
        ..Sources::default()
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
fn the_repo_manifest_turns_incremental_on_without_ci() {
    let manifest = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../Cargo.toml"));
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
