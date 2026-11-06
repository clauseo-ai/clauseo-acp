//! The sandbox's wire before the host builds it (decision record 0051, PR-A): the query and both
//! commands fail closed as `command.invalid`, and no matter's row says it is protected.

use clauseo_protocol::{ErrorCode, HostCommand, HostQuery};

use crate::support::{Dir, FAKE, Scenario};

#[test]
fn the_protection_query_and_commands_are_not_built_and_rows_carry_none() {
    let dir = Dir::new("protection", &[(FAKE, Scenario::Bundled("prompt-stream"))]);
    let host = dir.start(0);
    let error = host.query(HostQuery::Protection).unwrap_err();
    assert_eq!(error.code(), ErrorCode::CommandInvalid, "{error}");
    assert_eq!(error.message(), "not built yet");
    for command in [
        HostCommand::RetryProtection {},
        HostCommand::RemoveSandboxSetup {},
    ] {
        let error = host.refused(command, ErrorCode::CommandInvalid);
        assert_eq!(error.message(), "not built yet");
    }
    let rows = host.matters();
    assert_eq!(rows.len(), 1, "the Scratch matter: {rows:?}");
    assert!(rows.iter().all(|row| row.protection.is_none()), "{rows:?}");
    host.shutdown();
}
