//! The app's version has two homes that cannot share it: `Cargo.toml` (the crate, and the
//! version a `v*` tag must name) and `tauri.conf.json` (the installers, the updater and
//! Settings > About). This test fails when they differ, or when the version is not plain
//! `X.Y.Z`: no pre-release or build suffix, which the NSIS installers and the updater's
//! `version:` check do not compare the way semantic versioning does.
//! `.github/workflows/release.yml` checks a tag against both.

/// The `version` of `tauri.conf.json`, or a message saying why there is none.
fn tauri_conf_version() -> Result<String, String> {
    let conf: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json"))
        .map_err(|error| format!("tauri.conf.json is not JSON: {error}"))?;
    conf.get("version")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| "tauri.conf.json has no string `version`".to_owned())
}

/// True when `version` is three dot-separated numbers without leading zeros.
fn is_plain_version(version: &str) -> bool {
    let parts: Vec<&str> = version.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|part| {
            !part.is_empty()
                && part.bytes().all(|b| b.is_ascii_digit())
                && (*part == "0" || !part.starts_with('0'))
        })
}

#[test]
fn version_matches_tauri_conf() {
    let crate_version = env!("CARGO_PKG_VERSION");
    let conf_version = tauri_conf_version().unwrap();
    assert_eq!(
        conf_version, crate_version,
        "crates/desktop/tauri.conf.json says {conf_version}, crates/desktop/Cargo.toml says \
         {crate_version}; bump both together"
    );
    assert!(
        is_plain_version(crate_version),
        "the app's version {crate_version} is not plain X.Y.Z"
    );
}

#[test]
fn only_plain_versions_pass() {
    for good in ["0.0.0", "0.1.0", "1.0.0", "10.20.30"] {
        assert!(is_plain_version(good), "{good}");
    }
    for bad in [
        "",
        "1",
        "1.0",
        "1.0.0.0",
        "1.0.0-beta.1",
        "1.0.0+build",
        "01.0.0",
        "v1.0.0",
    ] {
        assert!(!is_plain_version(bad), "{bad}");
    }
}
