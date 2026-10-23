use super::SystemEnv;
use crate::locations::Env;
use std::ffi::OsString;

#[test]
fn system_env_reads_the_process_environment() {
    // The test runner sets this for every test process.
    assert_eq!(
        SystemEnv.var("CARGO_MANIFEST_DIR"),
        Some(OsString::from(env!("CARGO_MANIFEST_DIR")))
    );
    assert_eq!(SystemEnv.var("CLAUSEO_SETTINGS_TEST_UNSET_VARIABLE"), None);
}
