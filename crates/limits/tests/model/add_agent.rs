//! `Limits::add_agent`: a plan set up after start-up joins the model loading, with its read.

use clauseo_limits::{Action, Limits};
use clauseo_protocol::{AgentKind, Freshness};

use crate::support::{SECOND, account, at_ms, done, five_hour, t0, usage, view_of};

const CLAUDE: AgentKind = AgentKind::Claude;
const CODEX: AgentKind = AgentKind::Codex;

#[test]
fn an_added_agent_loads_and_reads() {
    let (mut model, actions) = Limits::new(Vec::new(), &[], t0());
    assert_eq!(actions, vec![]);
    assert_eq!(
        model.add_agent(CLAUDE, at_ms(SECOND)),
        vec![Action::Read(CLAUDE), Action::ViewChanged]
    );
    assert_eq!(
        view_of(&model, CLAUDE, at_ms(SECOND)).freshness,
        Freshness::Loading
    );
    let actions = model.input(
        done(CLAUDE, usage(account(CLAUDE, vec![five_hour(10.0)]))),
        at_ms(2 * SECOND),
    );
    assert!(
        actions
            .iter()
            .any(|action| matches!(action, Action::Changed(_))),
        "{actions:?}"
    );
    assert_eq!(
        view_of(&model, CLAUDE, at_ms(2 * SECOND)).freshness,
        Freshness::Fresh
    );
}

#[test]
fn an_agent_it_has_is_not_added_again() {
    let (mut model, _) = Limits::new(Vec::new(), &[CLAUDE], t0());
    assert_eq!(model.add_agent(CLAUDE, at_ms(SECOND)), vec![]);
    assert_eq!(model.view(at_ms(SECOND)).0.len(), 1);
}

#[test]
fn a_second_agent_joins_after_the_first() {
    let (mut model, _) = Limits::new(Vec::new(), &[CLAUDE], t0());
    assert_eq!(
        model.add_agent(CODEX, at_ms(SECOND)),
        vec![Action::Read(CODEX), Action::ViewChanged]
    );
    let view = model.view(at_ms(SECOND));
    assert_eq!(
        view.0.keys().copied().collect::<Vec<_>>(),
        vec![CLAUDE, CODEX]
    );
}
