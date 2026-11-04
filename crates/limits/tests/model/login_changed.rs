//! `Input::LoginChanged` (decision record 0048): a read at once past any age, another after a
//! read already in flight, still none while the agent asks a question.

use clauseo_limits::{Action, Input, Limits};
use clauseo_protocol::AgentKind;

use crate::support::{SECOND, account, at, at_ms, done, five_hour, read_once, t0, usage};

const CLAUDE: AgentKind = AgentKind::Claude;
const CODEX: AgentKind = AgentKind::Codex;

fn login_changed(agent: AgentKind) -> Input {
    Input::LoginChanged { agent }
}

#[test]
fn a_login_change_reads_at_once_whatever_the_age() {
    let mut model = read_once(account(CLAUDE, vec![five_hour(10.0)]));
    assert_eq!(
        model.input(login_changed(CLAUDE), at_ms(SECOND)),
        vec![Action::Read(CLAUDE)]
    );
    // A second change while that read is in flight asks no second read now.
    assert_eq!(
        model.input(login_changed(CLAUDE), at_ms(2 * SECOND)),
        vec![]
    );
}

#[test]
fn a_login_change_during_a_read_reads_again_when_it_ends() {
    let (mut model, actions) = Limits::new(Vec::new(), &[CLAUDE], t0());
    assert_eq!(actions, vec![Action::Read(CLAUDE)]);
    assert_eq!(model.input(login_changed(CLAUDE), at_ms(SECOND)), vec![]);
    let actions = model.input(
        done(CLAUDE, usage(account(CLAUDE, vec![]))),
        at_ms(2 * SECOND),
    );
    assert!(actions.contains(&Action::Read(CLAUDE)), "{actions:?}");
    // That reread ends like any other: no third read follows it.
    let actions = model.input(
        done(CLAUDE, usage(account(CLAUDE, vec![]))),
        at_ms(3 * SECOND),
    );
    assert!(!actions.contains(&Action::Read(CLAUDE)), "{actions:?}");
}

#[test]
fn a_login_change_while_asking_waits_for_the_answer() {
    let mut model = read_once(account(CLAUDE, vec![five_hour(10.0)]));
    let asking = |asking| Input::Asking {
        agent: CLAUDE,
        asking,
    };
    assert_eq!(model.input(asking(true), at(1)), vec![]);
    assert_eq!(model.input(login_changed(CLAUDE), at(2)), vec![]);
    assert_eq!(
        model.input(asking(false), at(3)),
        vec![Action::Read(CLAUDE)]
    );
}

#[test]
fn a_login_change_for_an_agent_not_given_does_nothing() {
    let mut model = read_once(account(CLAUDE, vec![five_hour(10.0)]));
    assert_eq!(model.input(login_changed(CODEX), at(1)), vec![]);
}
