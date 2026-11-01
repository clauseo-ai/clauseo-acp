//! A signed-out answer: the account is forgotten in memory, nothing is stored, and the view
//! shows the sign-in-again state until the next good read.

use clauseo_limits::{Action, Input, Limits, LiveStatus, ReadOutcome};
use clauseo_protocol::{AccountLimits, AccountRef, AgentKind, Freshness};

use crate::support::{
    account, at, changed, done, five_hour, live, live_input, read_once, t0, usage, view_of,
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
        vec![Action::ViewChanged],
        "no Changed: the stored entry stays"
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
    assert_eq!(changed(&actions).account.0, "partner@example.com");
    assert_eq!(view_of(&model, CLAUDE, at(10)).freshness, Freshness::Fresh);
}
