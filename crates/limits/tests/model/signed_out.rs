//! A signed-out answer: the account is forgotten in memory and in the store, and the view shows
//! the sign-in-again state until the next good read. A good read of another account forgets
//! the agent's other stored accounts.

use clauseo_limits::{Action, Input, Limits, LiveStatus, ReadOutcome};
use clauseo_protocol::{
    AccountLimits, AccountRef, AgentKind, Billing, BillingProvider, Freshness, LimitSource,
};

use crate::support::{
    account, at, changed, changed_then_forget, done, five_hour, live, live_input, read_once, t0,
    usage, view_of,
};

const CLAUDE: AgentKind = AgentKind::Claude;

/// Starts another read of Claude at `now`.
fn try_again(model: &mut Limits, now: clauseo_protocol::Timestamp) {
    let actions = model.input(
        Input::TryAgain {
            agent: Some(CLAUDE),
        },
        now,
    );
    assert_eq!(actions, vec![Action::Read(CLAUDE)]);
}

#[test]
fn signed_out_with_no_earlier_account_shows_missing_scope_and_stores_nothing() {
    let (mut model, _) = Limits::new(Vec::new(), &[AgentKind::Codex], t0());
    let actions = model.input(done(AgentKind::Codex, ReadOutcome::SignedOut), at(1));
    assert_eq!(
        actions,
        vec![Action::ViewChanged],
        "no Changed: nothing is stored"
    );
    let view = view_of(&model, AgentKind::Codex, at(1));
    assert_eq!(view.freshness, Freshness::MissingScope);
    assert_eq!(view.windows, vec![]);
    assert_eq!(model.at_limit(AgentKind::Codex, t0()), None);
}

#[test]
fn signed_out_after_a_good_read_forgets_the_account_until_the_next_good_read() {
    let mut model = read_once(account(CLAUDE, vec![five_hour(100.0)]));
    try_again(&mut model, at(6));
    let actions = model.input(done(CLAUDE, ReadOutcome::SignedOut), at(7));
    assert_eq!(
        actions,
        vec![Action::Forget {
            agent: CLAUDE,
            keep: None
        }],
        "the stored entry goes"
    );
    let view = view_of(&model, CLAUDE, at(7));
    assert_eq!(view.freshness, Freshness::MissingScope);
    assert_eq!(view.windows, vec![]);
    assert_eq!(view.plan, None);
    assert_eq!(view.meter, None);
    assert_eq!(model.at_limit(CLAUDE, at(7)), None);
    let signal = live("five_hour", LiveStatus::Rejected, Some(100.0), None);
    assert_eq!(
        model.input(live_input(CLAUDE, signal), at(8)),
        vec![],
        "live is ignored"
    );

    try_again(&mut model, at(9));
    assert_eq!(
        model.input(done(CLAUDE, ReadOutcome::SignedOut), at(9)),
        vec![],
        "no change"
    );

    try_again(&mut model, at(10));
    let other = AccountLimits {
        account: AccountRef("partner@example.com".to_owned()),
        ..account(CLAUDE, vec![five_hour(5.0)])
    };
    let actions = model.input(done(CLAUDE, usage(other)), at(10));
    assert_eq!(
        changed(&actions).account.0,
        "partner@example.com",
        "nothing is held, so nothing else is forgotten"
    );
    assert_eq!(view_of(&model, CLAUDE, at(10)).freshness, Freshness::Fresh);
}

#[test]
fn a_good_read_of_another_account_forgets_the_agents_other_entries() {
    let mut model = read_once(account(CLAUDE, vec![five_hour(100.0)]));
    try_again(&mut model, at(6));
    let other = AccountLimits {
        account: AccountRef("partner@example.com".to_owned()),
        ..account(CLAUDE, vec![five_hour(5.0)])
    };
    let actions = model.input(done(CLAUDE, usage(other.clone())), at(7));
    let (stored, keep) = changed_then_forget(&actions);
    assert_eq!(stored.account, other.account);
    assert_eq!(keep, Some(&other.account));

    try_again(&mut model, at(8));
    let actions = model.input(done(CLAUDE, usage(other)), at(8));
    assert_eq!(
        changed(&actions).account.0,
        "partner@example.com",
        "the same account again forgets nothing"
    );
}

#[test]
fn a_cached_account_goes_when_the_first_read_names_an_api_key_with_no_checked_time() {
    let cached = account(CLAUDE, vec![five_hour(40.0)]);
    let (mut model, actions) = Limits::new(vec![cached], &[CLAUDE], t0());
    assert_eq!(actions, vec![Action::Read(CLAUDE)]);
    let key = AccountLimits {
        account: AccountRef("api-key".to_owned()),
        plan: None,
        billing: Billing::PayAsYouGo {
            via: BillingProvider::Anthropic,
        },
        windows: vec![],
        source: LimitSource::NotApplicable,
        ..account(CLAUDE, vec![])
    };
    let actions = model.input(done(CLAUDE, usage(key)), at(1));
    let (_, keep) = changed_then_forget(&actions);
    assert_eq!(
        keep,
        Some(&AccountRef("api-key".to_owned())),
        "the timed entry would outrank the key's at the next start"
    );
}
