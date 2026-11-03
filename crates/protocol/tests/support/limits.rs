//! Examples of the limits types for the golden snapshots: an account's limits and a view of two
//! agents' limits. Included by `snapshots.rs` next to `fixtures`, which it builds on.

use std::collections::BTreeMap;

use clauseo_protocol::{
    AccountLimits, AccountRef, AgentKind, AgentLimitsView, AtLimitBehaviour, Billing, BillingKind,
    BillingProvider, Freshness, LimitSource, LimitWindow, LimitWindowView, LimitsView, MeterTone,
    PlanLabel, Timestamp, WindowFreshness, WindowKind,
};

use crate::fixtures::finite;

/// An account's limits, with one window.
pub fn account_limits() -> AccountLimits {
    AccountLimits {
        agent: AgentKind::Claude,
        account: AccountRef("acct-1".to_owned()),
        plan: Some(PlanLabel("Max".to_owned())),
        billing: Billing::Plan,
        windows: vec![LimitWindow {
            id: "five_hour".to_owned(),
            kind: WindowKind::Session,
            label: "5-hour".to_owned(),
            used_percent: finite(82.5),
            resets_at: Some(Timestamp::new(1_760_000_000_000).unwrap()),
            duration_mins: Some(300),
            warning: true,
            model: None,
        }],
        at_limit: AtLimitBehaviour::Pauses,
        source: LimitSource::Fresh {
            checked_at: Timestamp::new(1_760_000_000_000).unwrap(),
        },
    }
}

/// The limits view of two agents: Claude on a plan, with a next read planned, and Codex on pay as
/// you go, billed by `OpenAI`, so both new fields are set.
pub fn limits_view() -> LimitsView {
    let claude = AgentLimitsView {
        plan: Some("Max".to_owned()),
        billing: BillingKind::Plan,
        meter: None,
        windows: vec![LimitWindowView {
            id: "five_hour".to_owned(),
            kind: WindowKind::Session,
            label: "5-hour".to_owned(),
            model: None,
            used_percent: Some(finite(82.5)),
            tone: MeterTone::Near,
            resets_at: Some(Timestamp::new(1_760_000_000_000).unwrap()),
            freshness: WindowFreshness::Fresh,
        }],
        at_limit_text: "When you reach a limit, Claude pauses until it resets.".to_owned(),
        freshness: Freshness::Fresh,
        checked_at: Some(Timestamp::new(1_759_999_000_000).unwrap()),
        next_read_at: Some(Timestamp::new(1_760_000_000_000).unwrap()),
        provider: None,
    };
    let codex = AgentLimitsView {
        plan: None,
        billing: BillingKind::PayAsYouGo,
        meter: None,
        windows: vec![],
        at_limit_text: "This account pays as it goes, so no plan limits apply.".to_owned(),
        freshness: Freshness::NotApplicable,
        checked_at: None,
        next_read_at: None,
        provider: Some(BillingProvider::OpenAi),
    };
    LimitsView(BTreeMap::from([
        (AgentKind::Claude, claude),
        (AgentKind::Codex, codex),
    ]))
}
