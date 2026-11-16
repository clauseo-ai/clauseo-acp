//! `RetryAgentInstall` restarts only a failed, waiting or paused install: a missing agent is
//! installed only through `InstallAgent` or a sign-in pressed before its install.

use clauseo_agents::InstallState;
use clauseo_protocol::{CommandOutcome, ErrorCode};
use serde_json::json;

use super::fakes::{command, rig};
use super::{MAIN, claude};
use crate::bridge::lock;
use crate::test_support::block_on;

#[test]
fn retry_on_a_missing_agent_installs_nothing_and_asks_nothing() {
    let rig = rig(true);
    let retry = || {
        command(
            "retry_agent_install",
            &claude(),
            &json!({ "use_system_proxy": false }),
        )
    };
    for state in [
        InstallState::Ready {
            version: "1.0.0".to_owned(),
        },
        InstallState::Missing {
            download_bytes: 41_500_001,
        },
        InstallState::Installing,
    ] {
        lock(&rig.agents.states).insert(claude(), state.clone());
        let answer = block_on(rig.setup().run(MAIN, retry()));
        assert_eq!(
            answer.unwrap_err().code(),
            ErrorCode::CommandInvalid,
            "{state:?}"
        );
    }
    assert_eq!(rig.calls(), [], "no install, no retry");
    assert_eq!(*lock(&rig.confirm.asked), [], "no confirm");

    lock(&rig.agents.states).insert(claude(), InstallState::Paused);
    assert_eq!(
        block_on(rig.setup().run(MAIN, retry())),
        Ok(CommandOutcome::Done)
    );
    assert_eq!(rig.calls(), [("retry", claude())]);
}
