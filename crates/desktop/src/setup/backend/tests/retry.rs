//! `RetryAgentInstall` asks nothing, so it restarts only a failed, waiting or paused install: a
//! missing agent is installed through `InstallAgent`'s confirm alone.

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
        None,
        Some(InstallState::Missing {
            download_bytes: 41_500_001,
        }),
        Some(InstallState::Installing),
    ] {
        if let Some(state) = state.clone() {
            lock(&rig.agents.states).insert(claude(), state);
        }
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
