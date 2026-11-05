//! The environment a reader runs in: `host`'s rule for an agent, with no hook.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};

/// The spec's env map: the `inherited` environment (read at each read) minus every `GIT_*` and
/// `SSH_*` variable, the names in [`DROPPED_INHERITED`] and every name the agent's settings
/// variables (`own`) also name, then `own`, all ignoring ASCII case. Every name in `dropped`
/// (the Codex reader's [`CODEX_DROPPED`]) is left out of both.
///
/// The map is the child's whole environment: the host's launcher clears the environment. The
/// inherited variables are also in the map because `process`'s redactor takes its secrets only from
/// the spec's env, so every inherited key or token stays out of the stderr tail. On Windows `PATH`
/// and `Path` are one variable, and an inherited `Path` left beside the agent's `PATH` would win.
/// Never `Debug`-print the map: it holds those secrets. The rule is
/// `crates/host/src/agent/spec.rs`'s.
///
/// `forced`, when given, goes in last: every inherited or settings variable of its name,
/// ignoring ASCII case, is dropped first, so neither can override it. The Codex reader forces
/// `CODEX_HOME` to Clauseo's own Codex home this way, so nothing points it at `~/.codex`.
pub(crate) fn spec_env(
    own: &BTreeMap<String, String>,
    inherited: impl Iterator<Item = (OsString, OsString)>,
    forced: Option<(&OsStr, &OsStr)>,
    dropped: &[&str],
) -> BTreeMap<OsString, OsString> {
    let drops = |name: &OsStr| dropped.iter().any(|drop| same(OsStr::new(drop), name));
    let mut env: BTreeMap<OsString, OsString> = inherited
        .filter(|(name, _)| {
            !is_dropped_inherited(name)
                && !drops(name)
                && !own.keys().any(|own| same(OsStr::new(own), name))
        })
        .collect();
    env.extend(
        own.iter()
            .map(|(name, value)| (OsString::from(name), OsString::from(value)))
            .filter(|(name, _)| !drops(name)),
    );
    if let Some((name, value)) = forced {
        env.retain(|key, _| !same(key, name));
        env.insert(name.to_owned(), value.to_owned());
    }
    env
}

/// The prefixes of the inherited variables no reader gets: `GIT_*` would point git at another
/// repository; `SSH_*` would mark a remote session or lend the lawyer's ssh keys. The same list as
/// `crates/host/src/agent/spec.rs`'s (`limits` cannot depend on `host`).
const DROPPED_INHERITED_PREFIXES: [&str; 2] = ["GIT_", "SSH_"];

/// The inherited variables no reader gets, each a headless or remote marker: the same list as
/// `crates/host/src/agent/spec.rs`'s `DROPPED_INHERITED`.
pub(crate) const DROPPED_INHERITED: [&str; 2] = ["NO_BROWSER", "CLAUDE_CODE_REMOTE"];

/// The names left out of the Codex reader's environment wherever they come from: the same list
/// as `crates/host/src/agent/codex.rs`'s `DROPPED_VARS`.
pub(crate) const CODEX_DROPPED: [&str; 4] = [
    "CODEX_PATH",
    "APP_SERVER_LOGS",
    "DEFAULT_AUTH_REQUEST",
    "MODEL_PROVIDER",
];

/// Whether an inherited `name` stays out: a prefix in [`DROPPED_INHERITED_PREFIXES`] or a name
/// in [`DROPPED_INHERITED`], ignoring ASCII case.
fn is_dropped_inherited(name: &OsStr) -> bool {
    let bytes = name.as_encoded_bytes();
    DROPPED_INHERITED_PREFIXES.iter().any(|prefix| {
        bytes
            .get(..prefix.len())
            .is_some_and(|start| start.eq_ignore_ascii_case(prefix.as_bytes()))
    }) || DROPPED_INHERITED
        .iter()
        .any(|drop| same(OsStr::new(drop), name))
}

/// Puts the stored key (or a firm plan's variable, which goes in the same way) in the agent's
/// settings variables as `var` (decision record 0048): every
/// settings variable of that name in any case is dropped first, so an empty placeholder (the
/// installed entry's `ANTHROPIC_API_KEY: ""`) gives way. Never logged; the redactor takes it
/// from the spec's env, so a tail that echoes it shows `[redacted]`. Neither key variable is in
/// a drop list, so [`spec_env`]'s drops leave it in place
/// (`tests::a_stored_key_survives_every_drop`).
pub(crate) fn with_key(own: &mut BTreeMap<String, String>, var: &str, key: String) {
    own.retain(|name, _| !name.eq_ignore_ascii_case(var));
    own.insert(var.to_owned(), key);
}

/// One variable name on every OS: Windows ignores the case of env names.
fn same(a: &OsStr, b: &OsStr) -> bool {
    a.eq_ignore_ascii_case(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The lists match the host's (`crates/host/src/agent/spec.rs`, `agent/codex.rs`): a change
    /// to one goes in both.
    #[test]
    fn the_drop_lists_are_the_hosts() {
        assert_eq!(DROPPED_INHERITED_PREFIXES, ["GIT_", "SSH_"]);
        assert_eq!(DROPPED_INHERITED, ["NO_BROWSER", "CLAUDE_CODE_REMOTE"]);
        assert_eq!(
            CODEX_DROPPED,
            [
                "CODEX_PATH",
                "APP_SERVER_LOGS",
                "DEFAULT_AUTH_REQUEST",
                "MODEL_PROVIDER"
            ]
        );
    }

    #[test]
    fn inherited_markers_and_dropped_names_stay_out_in_any_case() {
        let env = spec_env(
            &own(&[("model_provider", "other"), ("KEEP", "1")]),
            os(&[
                ("GIT_DIR", "/elsewhere/.git"),
                ("ssh_auth_sock", "/tmp/agent.sock"),
                ("SSH_CONNECTION", "10.0.0.1 22 10.0.0.2 22"),
                ("No_Browser", "1"),
                ("CLAUDE_CODE_REMOTE", "true"),
                ("App_Server_Logs", "/tmp/logs"),
                ("SSHFS_OPTS", "kept"),
                ("PATH", "/usr/bin"),
            ])
            .into_iter(),
            None,
            &CODEX_DROPPED,
        );
        let names: Vec<&OsStr> = env.keys().map(OsString::as_os_str).collect();
        assert_eq!(names, ["KEEP", "PATH", "SSHFS_OPTS"].map(OsStr::new));
    }

    fn os(pairs: &[(&str, &str)]) -> Vec<(OsString, OsString)> {
        pairs
            .iter()
            .map(|(name, value)| (OsString::from(name), OsString::from(value)))
            .collect()
    }

    fn own(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect()
    }

    #[test]
    fn an_agent_variable_replaces_an_inherited_one_of_any_case() {
        let env = spec_env(
            &own(&[("PATH", "/agent/bin")]),
            os(&[("Path", "/inherited/bin"), ("HOME", "/home/lawyer")]).into_iter(),
            None,
            &[],
        );
        let names: Vec<&OsString> = env.keys().collect();
        assert_eq!(names, [OsStr::new("HOME"), OsStr::new("PATH")]);
        let expected: BTreeMap<OsString, OsString> =
            os(&[("HOME", "/home/lawyer"), ("PATH", "/agent/bin")])
                .into_iter()
                .collect();
        assert!(env == expected, "env differs");
    }

    #[test]
    fn inherited_values_are_kept_as_they_are() {
        let env = spec_env(
            &own(&[]),
            os(&[("LANG", "en_GB.UTF-8"), ("EMPTY", ""), ("SPACED", " a b ")]).into_iter(),
            None,
            &[],
        );
        assert_eq!(
            env.get(OsStr::new("LANG")),
            Some(&OsString::from("en_GB.UTF-8"))
        );
        assert_eq!(env.get(OsStr::new("EMPTY")), Some(&OsString::new()));
        assert_eq!(
            env.get(OsStr::new("SPACED")),
            Some(&OsString::from(" a b "))
        );
    }

    #[test]
    fn inherited_secrets_stay_in_the_map_for_the_redactor() {
        // Built at run time, so no file holds a token-shaped text.
        let openai = format!("sk{}", "-inherited-secret-value");
        let github = format!("ghp{}", "_inherited");
        let env = spec_env(
            &own(&[("CLAUDE_CONFIG_DIR", "/agent/config")]),
            os(&[("OPENAI_API_KEY", &openai), ("GH_TOKEN", &github)]).into_iter(),
            None,
            &[],
        );
        assert!(
            env.get(OsStr::new("OPENAI_API_KEY")) == Some(&OsString::from(&openai)),
            "OPENAI_API_KEY differs"
        );
        assert!(
            env.get(OsStr::new("GH_TOKEN")) == Some(&OsString::from(&github)),
            "GH_TOKEN differs"
        );
        assert_eq!(
            env.get(OsStr::new("CLAUDE_CONFIG_DIR")),
            Some(&OsString::from("/agent/config"))
        );
    }

    #[test]
    fn a_forced_variable_is_the_only_one_of_its_name_in_any_case() {
        let env = spec_env(
            &own(&[
                ("codex_home", "/settings/codex"),
                ("CODEX_HOME", "/settings/CODEX"),
            ]),
            os(&[
                ("CODEX_HOME", "/home/lawyer/.codex"),
                ("Codex_Home", "/inherited/other"),
                ("HOME", "/home/lawyer"),
            ])
            .into_iter(),
            Some((OsStr::new("CODEX_HOME"), OsStr::new("/data/codex-home"))),
            &[],
        );
        let homes: Vec<_> = env
            .iter()
            .filter(|(name, _)| name.eq_ignore_ascii_case("codex_home"))
            .collect();
        assert_eq!(
            homes,
            [(
                &OsString::from("CODEX_HOME"),
                &OsString::from("/data/codex-home")
            )]
        );
        assert_eq!(
            env.get(OsStr::new("HOME")),
            Some(&OsString::from("/home/lawyer"))
        );
    }

    /// The stored key's variables are in no drop list: `spec_env` keeps each one, from the
    /// settings side, under the Codex reader's drops too.
    #[test]
    fn a_stored_key_survives_every_drop() {
        for var in ["ANTHROPIC_API_KEY", "CODEX_API_KEY"] {
            assert!(!is_dropped_inherited(OsStr::new(var)), "{var}");
            assert!(
                !CODEX_DROPPED
                    .iter()
                    .any(|drop| drop.eq_ignore_ascii_case(var)),
                "{var}"
            );
            let mut settings = own(&[(var, "")]);
            with_key(&mut settings, var, "stored".to_owned());
            let env = spec_env(&settings, std::iter::empty(), None, &CODEX_DROPPED);
            assert!(
                env.get(OsStr::new(var)) == Some(&OsString::from("stored")),
                "{var}"
            );
        }
    }
}
