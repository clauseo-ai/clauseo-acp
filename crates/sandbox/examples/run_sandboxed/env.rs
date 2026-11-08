//! The values `--env` and `--env-file` give the command: their parsing, the `--env-file`'s checks
//! and removal, and the command's environment. Errors name a key or a line number, never a value.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs::File;
use std::io::Read;
use std::path::Path;

use rustix::fs::{FileType, Mode, OFlags};
use rustix::io::Errno;
use rustix::process::geteuid;

/// The variable `--browser-sockets` sets, which `--env` and `--env-file` may not.
pub(super) const SOCKET_DIR_VAR: &str = "AGENT_BROWSER_SOCKET_DIR";

/// Most bytes an `--env-file` may hold.
const ENV_FILE_CAP: u64 = 64 * 1024;

/// Why `key` and `value` cannot be one variable, or `None` when they can. The reason
/// never names the value. (`=` cannot be in `key`: the caller splits at the first one.)
fn bad_pair(key: &str, value: &str) -> Option<&'static str> {
    if key.is_empty() {
        Some("needs a non-empty key before the '='")
    } else if key.contains('\0') || value.contains('\0') {
        Some("has a NUL byte")
    } else {
        None
    }
}

/// One `K=V` from `--env`: the key is what precedes the first `=`, and the value keeps any
/// `=` after it.
pub(super) fn parse_env_pair(pair: &str) -> Result<(String, String), String> {
    let Some((key, value)) = pair.split_once('=') else {
        return Err("--env needs K=V".to_owned());
    };
    match bad_pair(key, value) {
        Some(why) => Err(format!("--env {why}")),
        None => Ok((key.to_owned(), value.to_owned())),
    }
}

/// The variables in an `--env-file`'s text: one `K=V` a line, with no quoting or escapes.
/// Blank lines and lines starting with `#` are skipped, and a trailing `\r` is dropped. An
/// error names the line number only, never the line.
pub(super) fn parse_env_file_text(text: &str) -> Result<Vec<(String, String)>, String> {
    let mut pairs = Vec::new();
    for (index, line) in text.split('\n').enumerate() {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let number = index + 1;
        let Some((key, value)) = line.split_once('=') else {
            return Err(format!("--env-file line {number} is not K=V"));
        };
        match bad_pair(key, value) {
            Some(why) => return Err(format!("--env-file line {number} {why}")),
            None => pairs.push((key.to_owned(), value.to_owned())),
        }
    }
    Ok(pairs)
}

/// Reads at most `ENV_FILE_CAP` bytes from `from`; more is an error.
fn read_capped(from: impl Read) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    from.take(ENV_FILE_CAP + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("--env-file: {e}"))?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > ENV_FILE_CAP {
        return Err("--env-file is larger than 64 KiB".to_owned());
    }
    Ok(bytes)
}

/// Removes the `--env-file` at `path`, a link itself and never what it points to. A file
/// that stays would keep the secret, so a failure stops the run.
#[expect(
    clippy::disallowed_methods,
    reason = "the --env-file is removed right after it is read, so no secret outlives the read"
)]
fn remove_secret(path: &Path) -> Result<(), String> {
    std::fs::remove_file(path).map_err(|e| format!("--env-file could not be removed: {e}"))
}

/// Reads the `--env-file` at `path`: a regular file owned by us with no group or other
/// bits, opened without following a link. Once it passes those checks it is removed, before
/// anything is parsed. A file that fails them is left alone.
pub(super) fn read_env_file(path: &Path) -> Result<String, String> {
    const REFUSED: &str = "--env-file must be a regular file only you can read";
    // O_NONBLOCK keeps a FIFO from holding the open; the fstat below refuses it.
    let flags = OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC;
    let fd = rustix::fs::open(path, flags, Mode::empty()).map_err(|e| match e {
        Errno::LOOP => REFUSED.to_owned(),
        other => format!("--env-file: {other}"),
    })?;
    let stat = rustix::fs::fstat(&fd).map_err(|e| format!("--env-file: {e}"))?;
    let regular = FileType::from_raw_mode(stat.st_mode) == FileType::RegularFile;
    let group_or_other = stat.st_mode & 0o077;
    if !regular || group_or_other != 0 || stat.st_uid != geteuid().as_raw() {
        return Err(REFUSED.to_owned());
    }
    let mut file = File::from(fd);
    let bytes = read_capped(&mut file);
    // The file is ours and passed its checks, so it is spent whatever the read found.
    remove_secret(path)?;
    String::from_utf8(bytes?).map_err(|e| format!("--env-file is not UTF-8: {}", e.utf8_error()))
}

/// The command's environment: the `--env-file` variables, then the `--env` pairs, so a
/// later key wins. With `--browser-sockets` the folder's variable is set by the caller,
/// so a given one is a refusal.
pub(super) fn launch_env(
    from_file: Vec<(String, String)>,
    from_args: Vec<(String, String)>,
    browser_sockets: bool,
) -> Result<BTreeMap<OsString, OsString>, String> {
    let mut env = BTreeMap::new();
    for (key, value) in from_file.into_iter().chain(from_args) {
        if browser_sockets && key == SOCKET_DIR_VAR {
            return Err(format!(
                "--browser-sockets sets {SOCKET_DIR_VAR}; drop it from --env or --env-file"
            ));
        }
        env.insert(OsString::from(key), OsString::from(value));
    }
    Ok(env)
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use super::*;

    fn pair(key: &str, value: &str) -> (String, String) {
        (key.to_owned(), value.to_owned())
    }

    #[test]
    fn an_env_pair_splits_at_the_first_equals() {
        assert_eq!(parse_env_pair("FOO=bar").unwrap(), pair("FOO", "bar"));
        assert_eq!(parse_env_pair("FOO=").unwrap(), pair("FOO", ""));
        assert_eq!(parse_env_pair("FOO=a=b=").unwrap(), pair("FOO", "a=b="));
    }

    #[test]
    fn a_bad_env_pair_is_refused_without_its_value() {
        for bad in ["FOO", "=bar", "FOO\0=1", "FOO=secret\0x"] {
            let why = parse_env_pair(bad).expect_err(bad);
            assert!(!why.contains("secret"), "{why}");
            assert!(why.starts_with("--env "), "{why}");
        }
    }

    #[test]
    fn env_file_lines_are_pairs_with_blanks_and_comments_skipped() {
        let text = "\n# a note\n   \nFOO=bar\n#BAR=no\nBAZ=a=b\n";
        assert_eq!(
            parse_env_file_text(text).unwrap(),
            vec![pair("FOO", "bar"), pair("BAZ", "a=b")]
        );
    }

    #[test]
    fn env_file_crlf_lines_lose_their_carriage_return() {
        assert_eq!(
            parse_env_file_text("FOO=bar\r\nBAZ=1\r").unwrap(),
            vec![pair("FOO", "bar"), pair("BAZ", "1")]
        );
    }

    #[test]
    fn a_bad_env_file_line_is_named_by_number_only() {
        for (text, line) in [
            ("A=1\nNOEQUALS\n", 2),
            ("A=1\n=secret\n", 2),
            ("A=1\n\nB=s\0x\n", 3),
        ] {
            let why = parse_env_file_text(text).expect_err(text);
            assert!(why.contains(&format!("line {line}")), "{why}");
            assert!(
                !why.contains("secret") && !why.contains("NOEQUALS"),
                "{why}"
            );
        }
    }

    #[test]
    fn the_env_file_cap_is_64_kib() {
        let at_cap = vec![b'a'; 64 * 1024];
        assert_eq!(read_capped(at_cap.as_slice()).unwrap().len(), 64 * 1024);
        let over = vec![b'a'; 64 * 1024 + 1];
        assert!(read_capped(over.as_slice()).is_err());
    }

    #[test]
    fn the_last_of_a_key_wins_and_args_beat_the_file() {
        let args = vec![pair("FOO", "a"), pair("FOO", "b")];
        let env = launch_env(Vec::new(), args, false).unwrap();
        assert_eq!(env.get(OsStr::new("FOO")), Some(&OsString::from("b")));
        let env = launch_env(vec![pair("FOO", "1")], vec![pair("FOO", "2")], false).unwrap();
        assert_eq!(env.get(OsStr::new("FOO")), Some(&OsString::from("2")));
    }

    #[test]
    fn browser_sockets_refuses_a_given_socket_dir() {
        let given = vec![pair(SOCKET_DIR_VAR, "x")];
        assert!(launch_env(Vec::new(), given.clone(), true).is_err());
        assert!(launch_env(given.clone(), Vec::new(), true).is_err());
        assert!(launch_env(Vec::new(), given, false).is_ok());
    }
}
