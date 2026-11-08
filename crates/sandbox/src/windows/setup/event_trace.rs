//! The setup exe's `tracing` lines at warn and above, to the Application Event Log (source
//! "Clauseo Sandbox", `refresh::event_log`): the exe has no other log output. Each line goes as
//! `event_line.rs` words it: its fixed message (withheld if it names a path or an account), where
//! it was logged, and its whole-number fields; never a text field, which carries errors that
//! may name a path or a SID. `--refresh` and `--stop` write their own lines, so `exe::main`
//! installs this for the other modes only.

use std::fmt;

use tracing::field::{Field, Visit};
use tracing::level_filters::LevelFilter;
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Level, Metadata, Subscriber};

use super::event_line::event_line;
use crate::windows::refresh::event_log;

/// The subscriber: events at warn and above, no spans.
struct EventLogTrace;

/// An event's message and its whole-number fields, its other fields dropped.
#[derive(Default)]
struct Message(String, Vec<(&'static str, i128)>);

impl Visit for Message {
    fn record_i64(&mut self, field: &Field, value: i64) {
        self.1.push((field.name(), i128::from(value)));
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.1.push((field.name(), i128::from(value)));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            value.clone_into(&mut self.0);
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        if field.name() == "message" {
            self.0 = format!("{value:?}");
        }
    }
}

impl Subscriber for EventLogTrace {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.is_event() && *metadata.level() <= Level::WARN
    }

    fn max_level_hint(&self) -> Option<LevelFilter> {
        Some(LevelFilter::WARN)
    }

    fn new_span(&self, _span: &Attributes<'_>) -> Id {
        Id::from_u64(1)
    }

    fn record(&self, _span: &Id, _values: &Record<'_>) {}

    fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

    fn event(&self, event: &Event<'_>) {
        let mut message = Message::default();
        event.record(&mut message);
        let metadata = event.metadata();
        let error = *metadata.level() == Level::ERROR;
        let module = metadata.module_path().unwrap_or("setup");
        event_log(
            error,
            &event_line(&message.0, module, metadata.line(), &message.1),
        );
    }

    fn enter(&self, _span: &Id) {}

    fn exit(&self, _span: &Id) {}
}

/// Sends this process's `tracing` lines to the Event Log from here on; once only.
pub(super) fn install() {
    let _already_set = tracing::subscriber::set_global_default(EventLogTrace);
}
