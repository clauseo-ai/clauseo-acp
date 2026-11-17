//! What a Claude session's flag settings keep Claude Code from loading as memory
//! ([`memory_settings`]): `CLAUDE.md` and `rules` in Clauseo's home for it, which Claude Code
//! reads again after a compaction, by `claudeMdExcludes`; and its auto-memory, by
//! `autoMemoryEnabled: false`, which also keeps a helper's `agent-memory` out. Measured on
//! Claude Code 2.1.293 (stage B, hot-reload part 3).
//!
//! It costs the lawyer nothing they have today: the data fence already denies Claude's tools
//! the memory folders of its projects (`data_fence::DENIED`), and Clauseo's own memory for the
//! agent is each matter's notes folder (decision record 0030).

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

/// The variable that, `0` or `false`, turns Claude Code's auto-memory on over the settings; the
/// launch sets it to `1` (`agent::home::claude_launch_env`, and the flag settings' `env`).
pub(crate) const AUTO_MEMORY_OFF_VAR: &str = "CLAUDE_CODE_DISABLE_AUTO_MEMORY";

/// The memory files under Claude's home that `claudeMdExcludes` names, as patterns under it.
const EXCLUDED: [&str; 2] = ["CLAUDE.md", "rules/**"];

/// Sets `claudeMdExcludes` and `autoMemoryEnabled: false` in the flag `settings`, and
/// [`AUTO_MEMORY_OFF_VAR`] `"1"` in their `env`. The excludes are absolute patterns, matched
/// against a file's absolute path: each of [`EXCLUDED`] under Claude's home `home` in each
/// spelling of the data folder (`spellings`, the canonical one and the configured one where they
/// differ, as the deny rules take them), with `\` as `/` too on Windows. None without a home.
pub(crate) fn memory_settings(
    settings: &mut Map<String, Value>,
    home: Option<&Path>,
    spellings: &[PathBuf],
) {
    let excludes: Vec<Value> = home
        .map(|home| home_spellings(home, spellings))
        .unwrap_or_default()
        .iter()
        .flat_map(|home| {
            EXCLUDED
                .iter()
                .filter_map(move |pattern| home.to_str().map(|home| format!("{home}/{pattern}")))
        })
        .flat_map(|pattern| {
            let slashed = pattern.replace('\\', "/");
            let both = cfg!(windows) && slashed != pattern;
            std::iter::once(pattern).chain(both.then_some(slashed))
        })
        .map(Value::from)
        .collect();
    if !excludes.is_empty() {
        settings.insert("claudeMdExcludes".to_owned(), Value::Array(excludes));
    }
    settings.insert("autoMemoryEnabled".to_owned(), Value::Bool(false));
    let mut env = match settings.remove("env") {
        Some(Value::Object(env)) => env,
        _ => Map::new(),
    };
    env.insert(AUTO_MEMORY_OFF_VAR.to_owned(), Value::from("1"));
    settings.insert("env".to_owned(), Value::Object(env));
}

/// `home` as it is, then under each other spelling of the data folder it lies in.
fn home_spellings(home: &Path, spellings: &[PathBuf]) -> Vec<PathBuf> {
    let mut homes = vec![home.to_path_buf()];
    let below = spellings
        .iter()
        .find_map(|data| home.strip_prefix(data).ok());
    for data in spellings {
        if let Some(below) = below {
            let other = data.join(below);
            if !homes.contains(&other) {
                homes.push(other);
            }
        }
    }
    homes
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use serde_json::{Map, Value, json};

    use super::memory_settings;

    /// The excludes name `CLAUDE.md` and `rules` under the home in each spelling of the data
    /// folder; auto-memory is off, in the settings and their `env`, an `env` already there kept.
    #[test]
    fn the_flag_settings_exclude_the_homes_memory_files_and_turn_auto_memory_off() {
        let base = if cfg!(windows) { "C:/" } else { "/" };
        let canonical = Path::new(base).join("real").join("data");
        let configured = Path::new(base).join("link").join("data");
        let home = configured.join("agents").join("claude-acp").join("home");
        let mut settings = Map::new();
        settings.insert("env".to_owned(), json!({"X": "1"}));
        memory_settings(&mut settings, Some(&home), &[canonical.clone(), configured]);
        let excludes: Vec<String> = settings["claudeMdExcludes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|pattern| pattern.as_str().unwrap().replace('\\', "/"))
            .collect();
        let real = canonical.join("agents").join("claude-acp").join("home");
        for home in [&home, &real] {
            let home = home.to_str().unwrap().replace('\\', "/");
            for pattern in [format!("{home}/CLAUDE.md"), format!("{home}/rules/**")] {
                assert!(excludes.contains(&pattern), "{pattern}: {excludes:?}");
            }
        }
        assert_eq!(settings["autoMemoryEnabled"], json!(false));
        assert_eq!(
            settings["env"],
            json!({"X": "1", "CLAUDE_CODE_DISABLE_AUTO_MEMORY": "1"})
        );
        let mut settings = Map::new();
        memory_settings(&mut settings, None, &[PathBuf::from(base)]);
        assert_eq!(settings.get("claudeMdExcludes"), None);
        assert_eq!(settings["autoMemoryEnabled"], Value::Bool(false));
    }
}
