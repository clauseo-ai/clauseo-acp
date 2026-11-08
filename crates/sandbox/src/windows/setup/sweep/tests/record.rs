//! The fake's record: the set-up lock, the `REG_QWORD` values and the clock (`Record`), and
//! the last run's values (`LastRun`).

use super::*;
use crate::windows::setup::record_values::{SWEEP_LAST_EXIT, SWEEP_LAST_SEQUENCE};

/// The fake's wall clock, as a `FILETIME`.
pub(super) const NOW: u64 = 133_000_000_000_000_000;

impl Record for Fake {
    fn lock(&mut self) -> Result<bool, Failed> {
        assert_eq!(self.held, 0, "taken once");
        self.done.push("lock".to_owned());
        self.held = u8::from(!self.busy);
        Ok(!self.busy)
    }

    fn unlock(&mut self) {
        self.held = 0;
    }

    fn read_value(&mut self, name: &'static str) -> Result<Option<u64>, Failed> {
        assert_eq!(self.held, 1, "read under the set-up lock");
        Ok(self.values.get(name).copied())
    }

    fn write_value(&mut self, name: &'static str, value: u64) -> Result<(), Failed> {
        assert_eq!(self.held, 1, "written under the set-up lock");
        if self.write_fails == Some(name) {
            return Err("RegSetValueExW failed (5)".to_owned());
        }
        self.values.insert(name, value);
        self.done.push(format!("set {name}"));
        Ok(())
    }

    fn now(&mut self) -> u64 {
        NOW
    }
}

impl LastRun for Fake {
    fn write_last_exit(&mut self, exit: u32) -> Result<(), Failed> {
        write_last(self, SWEEP_LAST_EXIT, u64::from(exit))
    }

    fn write_last_sequence(&mut self, sequence: u64) -> Result<(), Failed> {
        write_last(self, SWEEP_LAST_SEQUENCE, sequence)
    }
}

/// The last run's value `name`: written outside the set-up lock, before the log line.
fn write_last(fake: &mut Fake, name: &'static str, value: u64) -> Result<(), Failed> {
    assert_eq!(fake.held, 0, "written outside the set-up lock");
    assert!(fake.logged.is_empty(), "written before the log line");
    if fake.write_fails == Some(name) {
        return Err("RegSetValueExW failed (5)".to_owned());
    }
    fake.last.push((name, value));
    Ok(())
}
