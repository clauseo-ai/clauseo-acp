//! A Codex read whose `account/read` did not come (`AccountRead::NotRead`, the plan ref): the
//! merge keeps the last email for the same identity, and blanks it otherwise.

use clauseo_limits::{AccountRead, Action, Input, Limits, ReadOutcome};
use clauseo_protocol::{AccountLimits, AccountRef, AgentKind, PlanLabel, Timestamp};

use crate::support::{account, at, changed, done, five_hour, read_once, usage};

const CODEX: AgentKind = AgentKind::Codex;
/// The email the last good read gave.
const EMAIL: &str = "lawyer@example.com";
/// The ref of a plan account read without an email.
const REF_PLAN: &str = "plan";

/// A Codex plan account on `plan` with `account` as its ref and a 5-hour window.
fn codex(account_ref: &str, plan: &str) -> AccountLimits {
    AccountLimits {
        account: AccountRef(account_ref.to_owned()),
        plan: Some(PlanLabel(plan.to_owned())),
        ..account(CODEX, vec![five_hour(30.0)])
    }
}

/// A read of `limits` that did not read the account.
fn unread(limits: AccountLimits) -> ReadOutcome {
    ReadOutcome::Usage {
        limits,
        reached: None,
        account_read: AccountRead::NotRead,
    }
}

/// Asks for another read of Codex at `now`, then ends it with `outcome`; the account ref the
/// `Changed` carries.
fn read_again(model: &mut Limits, outcome: ReadOutcome, now: Timestamp) -> AccountRef {
    let actions = model.input(Input::TryAgain { agent: Some(CODEX) }, now);
    assert_eq!(actions, vec![Action::Read(CODEX)]);
    let actions = model.input(done(CODEX, outcome), now);
    changed(&actions).account.clone()
}

#[test]
fn a_late_account_read_keeps_the_last_email_for_the_same_plan() {
    let mut model = read_once(codex(EMAIL, "ChatGPT Plus"));
    let got = read_again(&mut model, unread(codex(REF_PLAN, "ChatGPT Plus")), at(1));
    assert_eq!(got, AccountRef(EMAIL.to_owned()));
}

#[test]
fn a_late_account_read_with_no_email_known_is_the_plan_ref() {
    let (mut model, actions) = Limits::new(Vec::new(), &[CODEX], at(0));
    assert_eq!(actions, vec![Action::Read(CODEX)]);
    let actions = model.input(done(CODEX, unread(codex(REF_PLAN, "ChatGPT Plus"))), at(0));
    assert_eq!(changed(&actions).account, AccountRef(REF_PLAN.to_owned()));
}

#[test]
fn a_late_account_read_on_another_plan_drops_the_email() {
    let mut model = read_once(codex(EMAIL, "ChatGPT Plus"));
    let got = read_again(&mut model, unread(codex(REF_PLAN, "ChatGPT Pro")), at(1));
    assert_eq!(got, AccountRef(REF_PLAN.to_owned()));
}

#[test]
fn an_account_read_that_says_no_email_is_not_kept_from_before() {
    let mut model = read_once(codex(EMAIL, "ChatGPT Go"));
    let got = read_again(&mut model, usage(codex(REF_PLAN, "ChatGPT Go")), at(1));
    assert_eq!(got, AccountRef(REF_PLAN.to_owned()));
}
