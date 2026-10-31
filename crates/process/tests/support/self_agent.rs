//! Starts this test binary again as the agent, running one test function by name with
//! [`ROLE`] set, so a test can have an agent do what no `fake_agent` scenario does (write the
//! stderr the test chose, leave its process group) with no helper binary. The function checks
//! [`is_agent`] first: in a normal run [`ROLE`] is unset and it returns at once. Input goes on
//! the agent's stdin, not in a variable (Windows caps one at 32,767 chars).

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;

use clauseo_process::{Error, LaunchSpec, Launched, SpawnOptions, spawn_in_tree};

/// The variable that makes a run of the test binary the agent.
const ROLE: &str = "CLAUSEO_PROCESS_TEST_AGENT_ROLE";

/// This test binary, started through [`spawn_in_tree`] with `opts`, to run only the test
/// function `name` (its path in the test crate, as `--exact` wants) as the agent. Its working
/// folder is this crate's, which it does not change.
pub(crate) fn launch(name: &str, opts: &SpawnOptions) -> Result<Launched, Error> {
    let program = std::env::current_exe().expect("the test binary's path");
    let args = ["--exact", name, "--nocapture", "--test-threads=1"]
        .map(OsString::from)
        .to_vec();
    let env = BTreeMap::from([(OsString::from(ROLE), OsString::from("1"))]);
    let spec = LaunchSpec::new(
        program,
        args,
        env,
        PathBuf::from(env!("CARGO_MANIFEST_DIR")),
    )?;
    spawn_in_tree(&spec, opts)
}

/// Whether this process is the agent, not a normal test run.
pub(crate) fn is_agent() -> bool {
    std::env::var_os(ROLE).is_some()
}
