//! The fake's record: the set-up lock, the `REG_QWORD` values and the clock (`Record`).

use super::*;

impl Record for Fake {
    fn lock(&mut self) -> Result<bool, Failed> {
        assert!(!self.held, "taken once");
        let busy = match self.marks {
            Marks::ReserveBusy => true,
            Marks::LockBusy => self.locks > 0,
            Marks::Written | Marks::WriteFails | Marks::CompletionFails => false,
        };
        self.locks += 1;
        self.held = !busy;
        Ok(self.held)
    }

    fn unlock(&mut self) {
        self.held = false;
    }

    fn now(&mut self) -> u64 {
        filetime(self.now)
    }

    fn read_value(&mut self, name: &'static str) -> Result<Option<u64>, Failed> {
        assert!(self.held, "read under the set-up lock");
        if self.read_fails == Some(name) {
            return Err("the record's value isn't a REG_QWORD".to_owned());
        }
        if self.gone() {
            return Ok(None);
        }
        Ok(self.values.get(name).copied())
    }

    fn write_value(&mut self, name: &'static str, value: u64) -> Result<(), Failed> {
        assert!(self.held, "written under the set-up lock");
        if self.gone() {
            return Err("the record's key is gone, so nothing was written".to_owned());
        }
        if self.marks == Marks::CompletionFails && name == STOP_COMPLETED {
            return Err("RegSetValueExW failed (5)".to_owned());
        }
        self.values.insert(name, value);
        self.done.push(format!("set {name}"));
        Ok(())
    }
}
