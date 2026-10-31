//! Starts this test binary again as the agent, running one test function by name with
//! [`ROLE`] set, so a test can have an agent do what no `fake_agent` scenario does (write the
//! exact stderr the test chose, leave its process group) with no helper binary. The function
//! checks [`role`] first: in a normal run [`ROLE`] is unset and it returns at once.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;

use clauseo_process::{Error, LaunchSpec, Launched, SpawnOptions, spawn_in_tree};

/// The variable that makes a run of the test binary the agent, holding its input.
const ROLE: &str = "CLAUSEO_PROCESS_TEST_AGENT_ROLE";

/// This test binary, started through [`spawn_in_tree`] with `opts`, to run only the test
/// function `name` (its path in the test crate, as `--exact` wants) with [`ROLE`] set to
/// `input`. Its working folder is this crate's, which it does not change.
pub(crate) fn launch(name: &str, input: &str, opts: &SpawnOptions) -> Result<Launched, Error> {
    let program = std::env::current_exe().expect("the test binary's path");
    let args = ["--exact", name, "--nocapture", "--test-threads=1"]
        .map(OsString::from)
        .to_vec();
    let env = BTreeMap::from([(OsString::from(ROLE), OsString::from(input))]);
    let spec = LaunchSpec::new(
        program,
        args,
        env,
        PathBuf::from(env!("CARGO_MANIFEST_DIR")),
    )?;
    spawn_in_tree(&spec, opts)
}

/// The agent's input when this process is the agent; `None` in a normal test run.
pub(crate) fn role() -> Option<String> {
    std::env::var(ROLE).ok()
}
