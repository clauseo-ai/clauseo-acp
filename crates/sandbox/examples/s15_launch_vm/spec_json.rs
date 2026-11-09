//! The launch spec a host builds, read from a JSON file for `exec-spec --spec-json`:
//! `{ "program": "<path>", "args": ["..."], "env": { "NAME": "value" } }`. `args` and `env` may
//! be left out. Unknown fields are an error, as is a missing or empty `program`.
//!
//! The parser is plain, so its tests run on every target; the launch itself is Windows-only
//! (`exec_spec.rs`).

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

/// The file's object, as serde reads it: an unknown key is refused.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    /// The program's path.
    program: String,
    /// Its arguments.
    #[serde(default)]
    args: Vec<String>,
    /// The environment; a map, so a name given twice is an error for the reader to see.
    #[serde(default)]
    env: BTreeMap<String, String>,
}

/// What to launch and its environment, as the file says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SpecJson {
    /// The program's path, as written.
    pub(crate) program: String,
    /// Its arguments.
    pub(crate) args: Vec<String>,
    /// The environment, by name in sorted order.
    pub(crate) env: Vec<(String, String)>,
}

/// Parses `text` as a launch spec; an error says what is wrong.
pub(crate) fn parse(text: &str) -> Result<SpecJson, String> {
    // Windows PowerShell 5's `Set-Content -Encoding UTF8` writes a byte-order mark first.
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let raw: Raw = serde_json::from_str(text).map_err(|e| format!("not a launch spec: {e}"))?;
    if raw.program.is_empty() {
        return Err("\"program\" is empty".to_owned());
    }
    Ok(SpecJson {
        program: raw.program,
        args: raw.args,
        env: raw.env.into_iter().collect(),
    })
}

/// The launch spec in the file at `path`; an error names the file and what is wrong.
pub(crate) fn load(path: &Path) -> Result<SpecJson, String> {
    let text =
        std::fs::read_to_string(path).map_err(|e| format!("can't read {}: {e}", path.display()))?;
    parse(&text).map_err(|why| format!("{}: {why}", path.display()))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{load, parse};

    #[test]
    fn valid_spec_parses() {
        let spec =
            parse(r#"{"program":"C:\\node\\node.exe","args":["-e","1"],"env":{"B":"2","A":"1"}}"#)
                .unwrap();
        assert_eq!(spec.program, r"C:\node\node.exe");
        assert_eq!(spec.args, ["-e", "1"]);
        assert_eq!(
            spec.env,
            [
                ("A".to_owned(), "1".to_owned()),
                ("B".to_owned(), "2".to_owned())
            ]
        );
    }

    #[test]
    fn args_and_env_may_be_left_out() {
        let spec = parse(r#"{"program":"/bin/x"}"#).unwrap();
        assert!(spec.args.is_empty());
        assert!(spec.env.is_empty());
    }

    #[test]
    fn byte_order_mark_is_skipped() {
        let spec = parse("\u{feff}{\"program\":\"/bin/x\"}").unwrap();
        assert_eq!(spec.program, "/bin/x");
    }

    #[test]
    fn unknown_field_is_refused() {
        let why = parse(r#"{"program":"/bin/x","cwd":"/tmp"}"#).unwrap_err();
        assert!(why.contains("cwd"), "{why}");
    }

    #[test]
    fn missing_program_is_refused() {
        let why = parse(r#"{"args":["-e"],"env":{}}"#).unwrap_err();
        assert!(why.contains("program"), "{why}");
    }

    #[test]
    fn empty_program_is_refused() {
        assert!(parse(r#"{"program":""}"#).is_err());
    }

    #[test]
    fn bad_json_is_refused() {
        assert!(parse(r#"{"program":"/bin/x""#).is_err());
    }

    #[test]
    fn missing_file_names_the_file() {
        let why = load(Path::new("no-such-dir/no-such-spec.json")).unwrap_err();
        assert!(why.starts_with("can't read no-such-dir"), "{why}");
    }
}
