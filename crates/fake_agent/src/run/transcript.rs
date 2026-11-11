//! The `transcript_user` step: the line Claude Code writes to its session transcript when it
//! takes the lawyer's words in, `{"type":"user", ...}` holding them as a `text` block. The host
//! reads that line to confirm a steer claude-agent-acp answered `injected`; without it the steer
//! is not sent. The transcript is `<home>/projects/<project key of the cwd>/<session>.jsonl`,
//! with `<home>` the `CLAUDE_CONFIG_DIR` the host gives every Claude agent.
//!
//! The line is stamped [`STAMP`], far ahead, as the host's own tests stamp theirs: the agent reads
//! no clock, and a line stamped ahead is never earlier than the steer it answers.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use serde_json::json;

/// The variable that names Claude Code's config folder, its home.
pub(super) const HOME_VAR: &str = "CLAUDE_CONFIG_DIR";

/// Every line's `timestamp`.
const STAMP: &str = "2999-01-01T00:00:00.000Z";

/// The longest project key Claude Code uses whole; a longer one is cut and given a hash.
const MAX_KEY: usize = 200;

/// Appends a `user` line holding `text` to `session`'s transcript under `home`, for a session
/// started in `cwd`, making its folders. Fails for an empty home or a session id that is not a
/// plain file name, as the host reads no other.
pub(super) fn append_user(home: &Path, cwd: &str, session: &str, text: &str) -> Result<(), String> {
    let path = path(home, cwd, session)?;
    let line = json!({
        "type": "user",
        "timestamp": STAMP,
        "sessionId": session,
        "cwd": cwd,
        "message": {"role": "user", "content": [{"type": "text", "text": text}]},
    });
    let mut bytes = line.to_string().into_bytes();
    bytes.push(b'\n');
    let parent = path.parent().unwrap_or(home);
    crate::write::create_dir_all(parent)
        .and_then(|()| crate::write::append(&path)?.write_all(&bytes))
        .map_err(|e| format!("transcript_user: write {}: {e}", path.display()))
}

/// `<home>/projects/<project key of cwd>/<session>.jsonl`.
fn path(home: &Path, cwd: &str, session: &str) -> Result<PathBuf, String> {
    if home.as_os_str().is_empty() {
        return Err(format!("transcript_user: {HOME_VAR} is not set"));
    }
    let plain = !session.is_empty()
        && session
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'));
    if !plain {
        return Err(format!(
            "transcript_user: session id `{session}` is not a plain file name"
        ));
    }
    Ok(home
        .join("projects")
        .join(project_key(cwd))
        .join(format!("{session}.jsonl")))
}

/// Claude Code's project key for a working folder `cwd`, as the host computes it
/// (`agent/data_fence/launch.rs`, `claude_project_key`): each UTF-16 unit that is not an ASCII
/// letter or digit made `-`; when that is longer than 200, its first 200, `-` and the base-36
/// hash of `cwd` (the absolute value of the 32-bit `h = 31 h + unit` over its UTF-16 units).
fn project_key(cwd: &str) -> String {
    let units: Vec<u16> = cwd.encode_utf16().collect();
    let key: String = units
        .iter()
        .map(|unit| match char::from_u32(u32::from(*unit)) {
            Some(c) if c.is_ascii_alphanumeric() => c,
            _ => '-',
        })
        .collect();
    if key.len() <= MAX_KEY {
        return key;
    }
    let hash = units.iter().fold(0i32, |hash, unit| {
        hash.wrapping_mul(31).wrapping_add(i32::from(*unit))
    });
    let mut left = u64::from(hash.unsigned_abs());
    let mut digits = Vec::new();
    loop {
        let digit = u32::try_from(left % 36).unwrap_or_default();
        digits.push(char::from_digit(digit, 36).unwrap_or('0'));
        left /= 36;
        if left == 0 {
            break;
        }
    }
    let hash: String = digits.into_iter().rev().collect();
    format!("{}-{hash}", key.get(..MAX_KEY).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The host's expected keys (`launch_tests.rs`), computed by Claude Code's own function.
    #[test]
    fn project_key_is_the_hosts() {
        assert_eq!(
            project_key("/Users/a/My Matter_1.x"),
            "-Users-a-My-Matter-1-x"
        );
        assert_eq!(
            project_key(r"C:\Users\A B\Matters\Acme"),
            "C--Users-A-B-Matters-Acme"
        );
        assert_eq!(project_key("/m/\u{1F600}"), "-m---");
        let long = format!(
            "/Users/a/Matters/{}\u{e9}\u{1F600}",
            "Acme v Widget Holdings \u{2013} Disclosure ".repeat(7)
        );
        let want = "-Users-a-Matters-Acme-v-Widget-Holdings---Disclosure-Acme-v-Widget-Holdings---\
                    Disclosure-Acme-v-Widget-Holdings---Disclosure-Acme-v-Widget-Holdings---\
                    Disclosure-Acme-v-Widget-Holdings---Disclosure-Acm-93gu6b";
        assert_eq!(project_key(&long), want);
    }

    #[test]
    fn path_needs_a_home_and_a_plain_session() {
        let home = Path::new("/home/claude");
        assert_eq!(
            path(home, "/m/Acme", "sess-1").unwrap(),
            home.join("projects").join("-m-Acme").join("sess-1.jsonl")
        );
        assert!(
            path(Path::new(""), "/m", "sess-1")
                .unwrap_err()
                .contains(HOME_VAR)
        );
        for session in ["", "../x", "a/b", r"a\b", "a.b"] {
            let error = path(home, "/m", session).unwrap_err();
            assert!(
                error.contains("not a plain file name"),
                "{session}: {error}"
            );
        }
    }
}
