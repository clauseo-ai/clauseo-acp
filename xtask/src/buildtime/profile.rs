//! Which incremental setting the timed builds run with, and where that setting came from.

/// Whether incremental compilation is on, and the place the answer came from, as people read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Incremental {
    pub on: bool,
    pub source: String,
}

/// Works out incremental from the environment and the root `Cargo.toml`, first match wins:
/// `CARGO_INCREMENTAL` (on unless `0`), `CARGO_PROFILE_DEV_INCREMENTAL` (`true` or `false`), the
/// `incremental` line of `[profile.dev]`, then cargo's default, which is on.
pub(crate) fn resolve(
    env_incremental: Option<&str>,
    env_dev: Option<&str>,
    manifest: &str,
) -> Incremental {
    if let Some(value) = env_incremental {
        return Incremental {
            on: value != "0",
            source: format!("CARGO_INCREMENTAL={value}"),
        };
    }
    match env_dev {
        Some("true") => {
            return Incremental {
                on: true,
                source: "CARGO_PROFILE_DEV_INCREMENTAL=true".to_owned(),
            };
        }
        Some("false") => {
            return Incremental {
                on: false,
                source: "CARGO_PROFILE_DEV_INCREMENTAL=false".to_owned(),
            };
        }
        _ => {}
    }
    match manifest_setting(manifest) {
        Some(on) => Incremental {
            on,
            source: format!("incremental = {on} in [profile.dev] of Cargo.toml"),
        },
        None => Incremental {
            on: true,
            source: "cargo default".to_owned(),
        },
    }
}

/// The `incremental` value in the `[profile.dev]` section of `manifest`. The section ends at the
/// next `[` header. Trailing comments are ignored.
pub(crate) fn manifest_setting(manifest: &str) -> Option<bool> {
    let mut in_dev = false;
    for raw in manifest.lines() {
        let line = raw.split('#').next().unwrap_or_default().trim();
        if line.starts_with('[') {
            in_dev = line == "[profile.dev]";
            continue;
        }
        if !in_dev {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim() != "incremental" {
            continue;
        }
        match value.trim() {
            "true" => return Some(true),
            "false" => return Some(false),
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEV_OFF: &str = "[profile.dev]\ndebug = 1\nincremental = false  # off\n";

    #[test]
    fn cargo_incremental_comes_first_and_is_on_unless_zero() {
        let zero = resolve(Some("0"), Some("true"), DEV_OFF);
        assert_eq!(
            zero,
            Incremental {
                on: false,
                source: "CARGO_INCREMENTAL=0".into()
            }
        );
        let one = resolve(Some("1"), Some("false"), DEV_OFF);
        assert_eq!(
            one,
            Incremental {
                on: true,
                source: "CARGO_INCREMENTAL=1".into()
            }
        );
    }

    #[test]
    fn the_profile_env_var_comes_before_the_manifest() {
        let off = resolve(None, Some("false"), "[profile.dev]\nincremental = true\n");
        assert_eq!(
            off,
            Incremental {
                on: false,
                source: "CARGO_PROFILE_DEV_INCREMENTAL=false".into()
            }
        );
        let on = resolve(None, Some("true"), DEV_OFF);
        assert_eq!(
            on,
            Incremental {
                on: true,
                source: "CARGO_PROFILE_DEV_INCREMENTAL=true".into()
            }
        );
    }

    #[test]
    fn an_unknown_profile_env_value_is_ignored() {
        let resolved = resolve(None, Some("maybe"), DEV_OFF);
        assert!(!resolved.on);
        assert_eq!(
            resolved.source,
            "incremental = false in [profile.dev] of Cargo.toml"
        );
    }

    #[test]
    fn the_manifest_comes_before_the_default() {
        let resolved = resolve(None, None, DEV_OFF);
        assert!(!resolved.on);
        assert_eq!(
            resolved.source,
            "incremental = false in [profile.dev] of Cargo.toml"
        );
    }

    #[test]
    fn the_default_is_on() {
        let resolved = resolve(None, None, "[profile.release]\nincremental = false\n");
        assert_eq!(
            resolved,
            Incremental {
                on: true,
                source: "cargo default".into()
            }
        );
    }

    #[test]
    fn the_profile_dev_section_ends_at_the_next_header() {
        let manifest = "[profile.dev]\ndebug = 1\n[profile.release]\nincremental = false\n";
        assert_eq!(manifest_setting(manifest), None);
        let trailing =
            "[profile.dev] # the dev profile\nincremental = true         # survives CI=1\n";
        assert_eq!(manifest_setting(trailing), Some(true));
    }

    #[test]
    fn the_repo_manifest_turns_incremental_on() {
        let manifest =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../Cargo.toml"));
        let manifest = manifest.unwrap_or_default();
        assert_eq!(manifest_setting(&manifest), Some(true));
    }
}
