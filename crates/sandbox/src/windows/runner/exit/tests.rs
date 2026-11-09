use std::collections::BTreeSet;

use super::*;

/// Every value ever shipped, with its variant's name. Append-only: a later PR adds its row
/// here when it moves its reservation into the enum, and never changes or removes a row, since
/// the host and anything downstream may read an old code.
const SHIPPED: [(u32, &str); 21] = [
    (0x00, "Usage"),
    (0x01, "BadLaunch"),
    (0x02, "NoSecret"),
    (0x03, "SecondaryLogon"),
    (0x04, "LogonRight"),
    (0x05, "Password"),
    (0x06, "Logon"),
    (0x07, "CodexReuse"),
    (0x08, "Pipe"),
    (0x09, "Job"),
    (0x0A, "Token"),
    (0x0B, "Spawn"),
    (0x0C, "AgentReserved"),
    (0x0D, "Sweep"),
    (0x0E, "DllSearch"),
    (0x0F, "Escapee"),
    (0x10, "PoolStop"),
    (0x11, "MarkerUnwritable"),
    (0x12, "AwaitingStop"),
    (0x13, "AwaitingSweep"),
    (0x16, "LogonPolicy"),
];

#[test]
fn every_code_is_its_own() {
    let codes: BTreeSet<u32> = RunExit::ALL.iter().map(|exit| exit.code()).collect();
    assert_eq!(codes.len(), RunExit::ALL.len());
    for exit in RunExit::ALL {
        assert_eq!(RunExit::from_code(exit.code()), Some(exit));
    }
}

#[test]
fn no_code_takes_a_reserved_value() {
    let reserved: BTreeSet<u32> = RunExit::RESERVED.iter().map(|(value, _)| *value).collect();
    assert_eq!(
        reserved.len(),
        RunExit::RESERVED.len(),
        "a value reserved twice"
    );
    for exit in RunExit::ALL {
        let value = exit.code() - RUN_EXIT_BASE;
        assert!(
            !reserved.contains(&value),
            "{exit:?} takes reserved {value:#x}"
        );
        assert_eq!(RunExit::from_code(RUN_EXIT_BASE + value), Some(exit));
    }
    for (value, _) in RunExit::RESERVED {
        assert_eq!(RunExit::from_code(RUN_EXIT_BASE + value), None);
    }
}

#[test]
fn the_shipped_codes_are_pinned_exactly() {
    let now: Vec<(u32, String)> = RunExit::ALL
        .iter()
        .map(|exit| (exit.code() - RUN_EXIT_BASE, format!("{exit:?}")))
        .collect();
    let shipped: Vec<(u32, String)> = SHIPPED
        .iter()
        .map(|(value, name)| (*value, (*name).to_owned()))
        .collect();
    assert_eq!(now, shipped);
    // The same numbers the index-based codes gave before the values were written out.
    assert_eq!(RunExit::Usage.code(), 0xE0C1_5000);
    assert_eq!(RunExit::AwaitingSweep.code(), 0xE0C1_5013);
    assert_eq!(RunExit::LogonPolicy.code(), 0xE0C1_5016);
}
