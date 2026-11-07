//! The verdict of the `child-token-default-dacl` row (`checks_pool::child_token`), from `probe
//! child-token`'s lines: no Win32 call here, so it has a unit test. The row records a fact and
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

#[cfg(test)]
mod tests {
    use super::{Verdict, child_token};

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
}
