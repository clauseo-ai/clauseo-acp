use clauseo_acp::{AcpEvent, AgentUpdate, Origin};
use clauseo_protocol::{
    AgentSessionId, CompactString, ConfigCategory, ConfigOption, ConfigOptionKind,
};

use super::{Reports, last_reported, reported};

fn option(category: ConfigCategory, current: &str) -> ConfigOption {
    ConfigOption {
        id: CompactString::from("o"),
        name: CompactString::from("o"),
        description: None,
        category,
        kind: ConfigOptionKind::Select {
            current_value: CompactString::from(current),
            options: Vec::new(),
        },
    }
}

fn mode(id: &str) -> AgentUpdate {
    AgentUpdate::CurrentMode {
        mode_id: CompactString::from(id),
    }
}

/// A `config_option_update` with the model option and, when given, the mode option at `mode`.
fn options(mode: Option<&str>) -> AgentUpdate {
    let mut options = vec![option(ConfigCategory::Model, "opus")];
    options.extend(mode.map(|mode| option(ConfigCategory::Mode, mode)));
    AgentUpdate::ConfigOptions(options)
}

fn event(update: AgentUpdate) -> AcpEvent {
    AcpEvent::Update {
        session: AgentSessionId::new("s1").unwrap(),
        origin: Origin::Idle,
        update,
    }
}

/// A `current_mode_update` and the mode option of a `config_option_update` report a mode; an
/// update without the mode option, or of any other kind, reports none.
#[test]
fn reported_reads_the_mode_update_and_the_mode_option() {
    assert_eq!(reported(&mode("plan")), Some("plan"));
    assert_eq!(reported(&options(Some("auto"))), Some("auto"));
    assert_eq!(reported(&options(None)), None);
    assert_eq!(reported(&AgentUpdate::Plan(Vec::new())), None);
    let boolean = ConfigOption {
        kind: ConfigOptionKind::Boolean {
            current_value: true,
        },
        ..option(ConfigCategory::Mode, "")
    };
    assert_eq!(reported(&AgentUpdate::ConfigOptions(vec![boolean])), None);
}

/// The last report counts, whichever kind it is; updates that report no mode are skipped, and a
/// list with no report has none.
#[test]
fn last_reported_takes_the_last_mode() {
    let switched = [mode("auto"), options(Some("auto")), options(None)].map(event);
    assert_eq!(last_reported(&switched), Some("auto"));
    let fell = [options(Some("auto")), mode("acceptEdits")].map(event);
    assert_eq!(last_reported(&fell), Some("acceptEdits"));
    let stale = [mode("plan"), options(Some("default"))].map(event);
    assert_eq!(last_reported(&stale), Some("default"));
    assert_eq!(last_reported(&[event(options(None))]), None);
    assert_eq!(last_reported(&[]), None);
}

/// A mark sees only the reports noted after it: none before, the last one after.
#[test]
fn reports_since_a_mark() {
    let mut reports = Reports::default();
    reports.note(&event(mode("plan")));
    let mark = reports.mark();
    assert_eq!(reports.since(mark), None);
    reports.note(&event(options(None)));
    assert_eq!(reports.since(mark), None);
    reports.note(&event(mode("acceptEdits")));
    reports.note(&event(options(Some("auto"))));
    assert_eq!(reports.since(mark), Some("auto"));
    assert_eq!(reports.since(reports.mark()), None);
}
