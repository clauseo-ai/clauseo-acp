//! Turning WER off through a fake registry: the values set and read back, and each failure
//! refusing (a link, a failed open, write or read, a value that reads back wrong).

use std::collections::BTreeMap;

use super::*;

/// One key's values; fails the calls it is told to.
#[derive(Default)]
struct Fake {
    values: BTreeMap<String, u32>,
    opened: Vec<String>,
    link: bool,
    fail_open: bool,
    /// Writes of this value fail.
    fail_write: Option<&'static str>,
    /// Reads of this value fail.
    fail_read: Option<&'static str>,
    /// Writes of this value are dropped: it reads back as it was.
    drop_write: Option<&'static str>,
}

impl Registry for Fake {
    type Key = ();
    fn open(&mut self, path: &str) -> Result<(), u32> {
        self.opened.push(path.to_owned());
        if self.fail_open { Err(5) } else { Ok(()) }
    }
    fn is_link(&mut self, (): &()) -> Result<bool, u32> {
        Ok(self.link)
    }
    fn set_dword(&mut self, (): &(), name: &str, value: u32) -> Result<(), u32> {
        if self.fail_write == Some(name) {
            return Err(5);
        }
        if self.drop_write != Some(name) {
            self.values.insert(name.to_owned(), value);
        }
        Ok(())
    }
    fn dword(&mut self, (): &(), name: &str) -> Result<Option<u32>, u32> {
        if self.fail_read == Some(name) {
            return Err(87);
        }
        Ok(self.values.get(name).copied())
    }
}

const KEY: &str = r"Software\Microsoft\Windows\Windows Error Reporting";

#[test]
fn the_per_user_wer_key_gets_disabled_and_dont_show_ui() {
    let mut fake = Fake::default();
    assert_eq!(quiet(&mut fake), Ok(()));
    assert_eq!(fake.opened, [KEY]);
    let set: Vec<(&str, u32)> = fake.values.iter().map(|(k, v)| (k.as_str(), *v)).collect();
    assert_eq!(set, [("Disabled", 1), ("DontShowUI", 1)]);
}

#[test]
fn setting_the_values_again_changes_nothing() {
    let mut fake = Fake::default();
    assert_eq!(quiet(&mut fake), Ok(()));
    let first = fake.values.clone();
    assert_eq!(quiet(&mut fake), Ok(()));
    assert_eq!(fake.values, first);
}

#[test]
fn a_key_that_is_a_link_is_refused_before_any_write() {
    let mut fake = Fake {
        link: true,
        ..Fake::default()
    };
    assert_eq!(quiet(&mut fake), Err(WerError::Link { key: KEY }));
    assert!(fake.values.is_empty());
}

#[test]
fn a_failed_open_is_refused() {
    let mut fake = Fake {
        fail_open: true,
        ..Fake::default()
    };
    assert_eq!(quiet(&mut fake), Err(WerError::Open { key: KEY, code: 5 }));
}

#[test]
fn a_failed_write_is_refused() {
    let mut fake = Fake {
        fail_write: Some("DontShowUI"),
        ..Fake::default()
    };
    assert_eq!(
        quiet(&mut fake),
        Err(WerError::Write {
            name: "DontShowUI",
            code: 5
        })
    );
}

#[test]
fn a_failed_read_back_is_refused() {
    let mut fake = Fake {
        fail_read: Some("Disabled"),
        ..Fake::default()
    };
    assert_eq!(
        quiet(&mut fake),
        Err(WerError::Read {
            name: "Disabled",
            code: 87
        })
    );
}

#[test]
fn a_value_that_reads_back_wrong_is_refused() {
    let mut fake = Fake {
        drop_write: Some("Disabled"),
        ..Fake::default()
    };
    fake.values.insert("Disabled".to_owned(), 0);
    assert_eq!(
        quiet(&mut fake),
        Err(WerError::ReadBack {
            name: "Disabled",
            found: Some(0)
        })
    );
    let mut missing = Fake {
        drop_write: Some("DontShowUI"),
        ..Fake::default()
    };
    assert_eq!(
        quiet(&mut missing),
        Err(WerError::ReadBack {
            name: "DontShowUI",
            found: None
        })
    );
}

#[test]
fn the_failure_names_the_sandbox_user_and_the_cause() {
    assert_eq!(
        WerError::Link { key: KEY }.to_string(),
        format!(
            "turning off Windows Error Reporting for the sandbox user failed: {KEY} is a \
             registry link"
        )
    );
}
