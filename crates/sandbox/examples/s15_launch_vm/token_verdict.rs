//! The verdict of the `child-token-default-dacl` row (`checks_pool::child_token`), from `probe
//! child-token`'s lines, and of the `job` row's owner check (`owners`): no Win32 call here, so
//! each has a unit test. The row records a fact and
//! doesn't gate: PASS whenever the child reported a result, carrying it; INFO only when no result
//! came back; never FAIL.
//!
//! The probe's lines: `token-adjust-default opened` or `token-adjust-default refused <code>`
//! (the child's), then `child-token exit <hex>`; or `child-token error <code>` when the agent
//! couldn't start its child.

use super::checks::values;
use super::pool_verdict::Verdict;

/// What the agent's child got from `OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_DEFAULT)`
/// on its own token: PASS carrying `opened` or `refused <code>`; INFO naming why no result came.
pub(crate) fn child_token(lines: &[String]) -> Verdict {
    if values(lines, "token-adjust-default").contains(&"opened") {
        return Verdict::Pass(
            "the agent's child opened its own token with TOKEN_ADJUST_DEFAULT (its token's DACL \
             comes from the agent's default DACL, which grants the pool user GA; any process can \
             shut its own DACL through its pseudo-handle anyway)"
                .to_owned(),
        );
    }
    if let Some(code) = values(lines, "token-adjust-default refused").first() {
        return Verdict::Pass(format!(
            "the agent's child was refused TOKEN_ADJUST_DEFAULT on its own token ({code})"
        ));
    }
    if let Some(code) = values(lines, "child-token error").first() {
        return Verdict::Info(format!(
            "the agent couldn't start its child ({code}), so no result came"
        ));
    }
    let exit = values(lines, "child-token exit")
        .first()
        .map_or_else(|| "no exit line".to_owned(), |code| format!("exit {code}"));
    Verdict::Info(format!(
        "the agent's child printed no token-adjust-default line ({exit}): {lines:?}"
    ))
}

/// The processes the runner starts as the pool user, whose owner must be the pool SID: `--stop`
/// knows a pool user's process by its owner, and kills no process another SID owns.
const POOL_OWNED: [&str; 3] = ["inner", "agent", "child"];

/// The `job` row's owner check, from the harness's `owner <role> <pid> <sid>` lines (`owner
/// <role> <pid> error <code>` when it couldn't be read, `owner <role> none` when no such process
/// was found), roles `inner`, `agent`, `child` and `outer`: FAIL naming each of the first three
/// whose owner isn't `pool` (or wasn't read), with its owner; PASS otherwise. The outer's owner
/// (the desktop user's) is information either way.
pub(crate) fn owners(pool: &str, lines: &[String]) -> Verdict {
    let found = values(lines, "owner");
    let of = |role: &str| {
        found.iter().find_map(|rest| {
            let (named, rest) = rest.split_once(' ')?;
            (named == role).then_some(rest)
        })
    };
    let mut bad = Vec::new();
    for role in POOL_OWNED {
        let Some(rest) = of(role) else {
            bad.push(format!("the {role}'s owner wasn't read"));
            continue;
        };
        let (pid, owner) = rest.split_once(' ').unwrap_or((rest, ""));
        if owner.eq_ignore_ascii_case(pool) {
            continue;
        }
        bad.push(match (pid, owner.strip_prefix("error ")) {
            ("none", _) => format!("no {role} process was found"),
            (_, Some(code)) => format!("the {role} {pid}'s owner couldn't be read ({code})"),
            (_, None) => format!("the {role} {pid} is owned by {owner}"),
        });
    }
    let outer = of("outer").map_or_else(
        || "the outer's owner wasn't read".to_owned(),
        |rest| format!("the outer (the desktop user's) {rest}"),
    );
    if bad.is_empty() {
        Verdict::Pass(format!(
            "the inner, the agent and its child are owned by the pool SID; information: {outer}"
        ))
    } else {
        Verdict::Fail(format!(
            "not owned by the pool SID {pool}: {}; information: {outer}",
            bad.join(", ")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::{Verdict, child_token, owners};

    fn lines(text: &[&str]) -> Vec<String> {
        text.iter().map(|l| (*l).to_owned()).collect()
    }

    fn passes(verdict: &Verdict, word: &str) {
        let Verdict::Pass(text) = verdict else {
            panic!("not PASS: {verdict:?}");
        };
        assert!(text.contains(word), "{text:?} doesn't name {word:?}");
    }

    fn info(verdict: &Verdict, word: &str) {
        let Verdict::Info(text) = verdict else {
            panic!("not INFO: {verdict:?}");
        };
        assert!(text.contains(word), "{text:?} doesn't name {word:?}");
    }

    #[test]
    fn child_token_passes_on_either_result_and_is_info_only_without_one() {
        let opened = child_token(&lines(&[
            "token-adjust-default opened",
            "child-token exit 0x0",
        ]));
        passes(&opened, "opened its own token");
        let refused = child_token(&lines(&[
            "token-adjust-default refused 5",
            "child-token exit 0x0",
        ]));
        passes(
            &refused,
            "was refused TOKEN_ADJUST_DEFAULT on its own token (5)",
        );
        info(
            &child_token(&lines(&["child-token error 5"])),
            "couldn't start its child (5)",
        );
        info(
            &child_token(&lines(&["child-token exit 0xc0000135"])),
            "no token-adjust-default line (exit 0xc0000135)",
        );
    }

    const POOL: &str = "S-1-5-21-1-2-3-1001";

    #[test]
    fn owners_fail_naming_each_process_the_pool_sid_doesnt_own() {
        let all = |inner: &'static str| {
            lines(&[
                inner,
                "owner agent 20 S-1-5-21-1-2-3-1001",
                "owner child 30 s-1-5-21-1-2-3-1001",
                "owner outer 5 S-1-5-21-1-2-3-500",
            ])
        };
        passes(
            &owners(POOL, &all("owner inner 10 S-1-5-21-1-2-3-1001")),
            "outer (the desktop user's) 5 S-1-5-21-1-2-3-500",
        );
        for (inner, named) in [
            (
                "owner inner 10 S-1-5-32-544",
                "the inner 10 is owned by S-1-5-32-544",
            ),
            (
                "owner inner 10 error 5",
                "the inner 10's owner couldn't be read (5)",
            ),
            ("owner inner none", "no inner process was found"),
            ("", "the inner's owner wasn't read"),
        ] {
            let Verdict::Fail(why) = owners(POOL, &all(inner)) else {
                panic!("not FAIL: {inner}");
            };
            assert!(why.contains(named), "{why}");
            assert!(!why.contains("agent"), "{why}");
        }
    }
}
