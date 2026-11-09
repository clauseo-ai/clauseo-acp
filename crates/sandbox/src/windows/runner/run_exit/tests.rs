//! The exit codes: stable, explicit where reserved, and mapped to set-up causes.

use super::super::{RUN_EXIT_BASE, SECLOGON_DISABLED};
use super::{RunExit, agent_code};

#[test]
fn exit_codes_are_stable_and_map_to_set_up_causes() {
    assert_eq!(RunExit::Usage.code(), 0xE0C1_5000);
    assert_eq!(RunExit::SecondaryLogon.code(), 0xE0C1_5003);
    assert_eq!(RunExit::Spawn.code(), 0xE0C1_500B);
    for exit in RunExit::ALL
        .into_iter()
        .chain([RunExit::TokenGrant, RunExit::PoolBusy])
    {
        assert_eq!(RunExit::from_code(exit.code()), Some(exit));
    }
    assert_eq!(RunExit::from_code(0), None);
    assert_eq!(RunExit::Sweep.code(), 0xE0C1_500D);
    assert_eq!(RunExit::DllSearch.code(), 0xE0C1_500E);
    // Appended last: every code before it keeps its number.
    assert_eq!(RunExit::Escapee.code(), 0xE0C1_500F);
    assert_eq!(RunExit::Escapee.no_sandbox(), None);
    assert_eq!(RunExit::PoolStop.code(), 0xE0C1_5010);
    assert_eq!(RunExit::PoolStop.no_sandbox(), None);
    assert_eq!(RunExit::MarkerUnwritable.code(), 0xE0C1_5011);
    assert_eq!(RunExit::MarkerUnwritable.no_sandbox(), None);
    assert_eq!(RunExit::AwaitingStop.code(), 0xE0C1_5012);
    assert_eq!(RunExit::AwaitingStop.no_sandbox(), None);
    assert_eq!(RunExit::AwaitingSweep.code(), 0xE0C1_5013);
    assert_eq!(RunExit::AwaitingSweep.no_sandbox(), None);
    // Reserved at 0x15; 0x14 is another branch's code, none here.
    assert_eq!(RunExit::TokenGrant.code(), 0xE0C1_5015);
    assert_eq!(RunExit::TokenGrant.no_sandbox(), None);
    // Reserved at 0x20; 0x16 to 0x1f are other branches' codes, none here.
    assert_eq!(RunExit::PoolBusy.code(), 0xE0C1_5020);
    assert_eq!(RunExit::PoolBusy.no_sandbox(), None);
    assert_eq!(RunExit::from_code(0xE0C1_5020), Some(RunExit::PoolBusy));
    for unknown in [0x14, 0x16, 0x1f, 0x21] {
        assert_eq!(
            RunExit::from_code(RUN_EXIT_BASE + unknown),
            None,
            "0x{unknown:x}"
        );
    }
    assert_eq!(RunExit::DllSearch.no_sandbox(), None);
    assert_eq!(RunExit::AgentReserved.no_sandbox(), None);
    assert_eq!(RunExit::Sweep.no_sandbox(), None);
    // An agent exiting with a runner code never passes for a runner failure.
    for code in [
        RunExit::Password.code(),
        RunExit::SecondaryLogon.code(),
        RUN_EXIT_BASE + 0xff,
    ] {
        assert_eq!(agent_code(code), RunExit::AgentReserved.code());
    }
    for code in [0, 7, RUN_EXIT_BASE - 1, RUN_EXIT_BASE + 0x100, 0xC000_0142] {
        assert_eq!(agent_code(code), code);
    }
    assert_eq!(
        RunExit::SecondaryLogon.no_sandbox(),
        Some(crate::NoSandbox::SetUpFailed(SECLOGON_DISABLED.to_owned()))
    );
    assert_eq!(
        RunExit::LogonRight.no_sandbox(),
        Some(crate::NoSandbox::SetUpBlocked(crate::Blocked::LogonRight))
    );
    assert_eq!(RunExit::Spawn.no_sandbox(), None);
    assert_eq!(SECLOGON_DISABLED, crate::windows::setup::SECLOGON_DISABLED);
}
