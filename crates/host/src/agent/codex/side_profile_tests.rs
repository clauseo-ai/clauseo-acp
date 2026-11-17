use std::path::Path;

use super::{home_config, side_profile_toml};

/// The switches `home_config_holds_only_the_switches` names, in order.
const SWITCHES: [&str; 8] = [
    "features.apps = false",
    "features.plugins = false",
    "features.skill_mcp_dependency_install = false",
    "cloud.skills.enabled = false",
    "skills.bundled.enabled = false",
    "skills.include_instructions = false",
    "cli_auth_credentials_store = \"file\"",
    "shell_environment_policy.ignore_default_excludes = false",
];

/// The home file on Unix: the switches, then `default_permissions` naming the side profile and
/// its table, read-only, the data folder denied, and nothing else.
#[test]
fn the_home_file_gives_side_threads_a_read_only_profile() {
    // unix-paths-ok: text in the home config only, never opened
    let locations = clauseo_settings::Locations::under_root(Path::new("/home/lawyer"));
    let text = format!("{}{}", home_config(), side_profile_toml(&locations, false));
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[2..10], SWITCHES, "{text}");
    assert_eq!(
        lines[10..],
        [
            "default_permissions = \"clauseo-side\"",
            "[permissions.clauseo-side]",
            "extends = \":read-only\"",
            "[permissions.clauseo-side.filesystem]",
            "\"/home/lawyer\" = \"deny\"",
        ],
        "{text}"
    );
    let parsed: toml::Table = toml::from_str(&text).unwrap();
    assert_eq!(parsed["default_permissions"].as_str(), Some("clauseo-side"));
    let profile = parsed["permissions"]["clauseo-side"].as_table().unwrap();
    assert_eq!(profile["extends"].as_str(), Some(":read-only"));
    assert_ne!(profile["extends"].as_str(), Some(":workspace"));
    let filesystem = profile["filesystem"].as_table().unwrap();
    let mut expected = toml::Table::new();
    // unix-paths-ok: text in the home config only, never opened
    expected.insert("/home/lawyer".to_owned(), toml::Value::from("deny"));
    assert_eq!(*filesystem, expected);
    assert_eq!(profile.len(), 2, "{text}");
    assert_eq!(parsed["permissions"].as_table().unwrap().len(), 1, "{text}");
    assert!(!parsed.contains_key("sandbox_mode"), "{text}");
    assert!(!parsed.contains_key("hooks"), "{text}");
    assert!(!text.contains("sandbox_mode"), "{text}");
    assert!(!text.contains("hooks"), "{text}");
}

/// A data folder whose path holds a quote, a backslash or a bracket is still one quoted key.
#[test]
fn the_side_profile_quotes_the_data_folder() {
    // unix-paths-ok: text in the home config only, never opened
    let root = Path::new("/home/la\"wy\\er [x]");
    let locations = clauseo_settings::Locations::under_root(root);
    let text = format!("{}{}", home_config(), side_profile_toml(&locations, false));
    let parsed: toml::Table = toml::from_str(&text).unwrap();
    let filesystem = parsed["permissions"]["clauseo-side"]["filesystem"]
        .as_table()
        .unwrap();
    assert_eq!(filesystem.len(), 1, "{text}");
    assert_eq!(filesystem[root.to_str().unwrap()].as_str(), Some("deny"));
}

/// On Windows the home file has no side profile, as the session's fence has none.
#[test]
fn the_side_profile_is_empty_on_windows() {
    // unix-paths-ok: text in the home config only, never opened
    let locations = clauseo_settings::Locations::under_root(Path::new("/home/lawyer"));
    assert_eq!(side_profile_toml(&locations, true), "");
}
