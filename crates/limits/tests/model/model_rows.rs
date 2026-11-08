//! Per-model rows and the stop: a Codex model bucket's row is shown but never stops the agent
//! or sets the toolbar's meter; a Claude per-model row does both.

use clauseo_limits::{LimitHit, Limits};
use clauseo_protocol::{AgentKind, LimitStopReason, LimitWindow, WindowKind};

use crate::support::{
    account, at, done, five_hour, read_once, resetting, row, t0, usage_reached, view_of, window,
};

const CLAUDE: AgentKind = AgentKind::Claude;
const CODEX: AgentKind = AgentKind::Codex;

/// A per-model row `id`, `used` percent used, resetting at minute `resets`.
fn model_row(id: &str, used: f64, resets: i64) -> LimitWindow {
    LimitWindow {
        model: Some("Spark".to_owned()),
        duration_mins: Some(300),
        ..resetting(window(id, WindowKind::Model, used), at(resets))
    }
}

#[test]
fn a_codex_model_row_at_its_limit_stops_nothing_and_a_claude_one_does() {
    let rows = |agent| {
        account(
            agent,
            vec![
                resetting(five_hour(40.0), at(60)),
                model_row("spark", 100.0, 30),
            ],
        )
    };
    assert_eq!(read_once(rows(CODEX)).at_limit(CODEX, t0()), None);
    let hit = LimitHit {
        window: Some("spark".to_owned()),
        resets_at: Some(at(30)),
        reason: LimitStopReason::PlanWindow,
    };
    assert_eq!(read_once(rows(CLAUDE)).at_limit(CLAUDE, t0()), Some(hit));
}

#[test]
fn a_reached_codex_window_times_out_with_codex_rows_and_names_one_of_them() {
    let codex_reached = |windows| {
        let (mut model, _) = Limits::new(Vec::new(), &[CODEX], t0());
        let read = usage_reached(account(CODEX, windows), LimitStopReason::PlanWindow);
        model.input(done(CODEX, read), t0());
        model
    };
    // The model row is the tighter one, but the hit names Codex's own row.
    let model = codex_reached(vec![
        resetting(five_hour(40.0), at(60)),
        model_row("spark", 90.0, 30),
    ]);
    let hit = LimitHit {
        window: Some("five_hour".to_owned()),
        resets_at: Some(at(60)),
        reason: LimitStopReason::PlanWindow,
    };
    assert_eq!(model.at_limit(CODEX, t0()), Some(hit));
    // Once Codex's own row is past its reset, a later model row keeps nothing reached.
    let model = codex_reached(vec![
        resetting(five_hour(40.0), at(1)),
        model_row("spark", 50.0, 600),
    ]);
    assert_eq!(model.at_limit(CODEX, at(2)), None);
}

#[test]
fn the_meter_reads_codex_rows_alone_and_the_card_shows_every_row() {
    let rows = |agent| {
        account(
            agent,
            vec![
                resetting(five_hour(40.0), at(60)),
                model_row("spark", 95.0, 30),
            ],
        )
    };
    let codex = view_of(&read_once(rows(CODEX)), CODEX, t0());
    let meter = codex.meter.as_ref().expect("a meter");
    assert_eq!(meter.window_label, "five_hour label");
    let spark = row(&codex, "spark");
    assert_eq!(
        (spark.model.as_deref(), spark.duration_mins),
        (Some("Spark"), Some(300))
    );
    let claude = view_of(&read_once(rows(CLAUDE)), CLAUDE, t0());
    let meter = claude.meter.as_ref().expect("a meter");
    assert_eq!(meter.window_label, "spark label");
}
