//! The environment a reader runs in: `host`'s rule for an agent, with no hook.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};

/// The spec's env map: the `inherited` environment (read at each read) minus every name the
/// agent's settings variables (`own`) also name, ignoring ASCII case, then `own`.
///
/// The child gets the same environment as with `own` alone, since it inherits the rest anyway.
/// The inherited variables are in the map because `process`'s redactor takes its secrets only from
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
) -> BTreeMap<OsString, OsString> {
    let mut env: BTreeMap<OsString, OsString> = inherited
        .filter(|(name, _)| !own.keys().any(|own| same(OsStr::new(own), name)))
        .collect();
    env.extend(
        own.iter()
            .map(|(name, value)| (OsString::from(name), OsString::from(value))),
    );
    if let Some((name, value)) = forced {
        env.retain(|key, _| !same(key, name));
        env.insert(name.to_owned(), value.to_owned());
    }
    env
}

/// One variable name on every OS: Windows ignores the case of env names.
fn same(a: &OsStr, b: &OsStr) -> bool {
    a.eq_ignore_ascii_case(b)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        );
        assert_eq!(
            env,
            os(&[("HOME", "/home/lawyer"), ("PATH", "/agent/bin")])
                .into_iter()
                .collect()
        );
    }

    #[test]
    fn inherited_values_are_kept_as_they_are() {
        let env = spec_env(
            &own(&[]),
            os(&[("LANG", "en_GB.UTF-8"), ("EMPTY", ""), ("SPACED", " a b ")]).into_iter(),
            None,
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
        let env = spec_env(
            &own(&[("CLAUDE_CONFIG_DIR", "/agent/config")]),
            os(&[
                ("OPENAI_API_KEY", "sk-inherited-secret-value"),
                ("GH_TOKEN", "ghp_inherited"),
            ])
            .into_iter(),
            None,
        );
        assert_eq!(
            env.get(OsStr::new("OPENAI_API_KEY")),
            Some(&OsString::from("sk-inherited-secret-value"))
        );
        assert_eq!(
            env.get(OsStr::new("GH_TOKEN")),
            Some(&OsString::from("ghp_inherited"))
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
}
