//! Tests of `run` in `setup.rs`: its exit code, through an injected runner on a scratch checkout.

use super::*;
use crate::scratch::Scratch;

#[test]
fn a_setup_whose_every_command_fails_exits_with_failure() {
    let root = Scratch::new("setup-run");
    let mut runner = |_: &Cmd, _: &Path, _: Output| -> io::Result<Ran> {
        Err(io::Error::other("no runner in this test"))
    };
    assert_eq!(
        run(&root, Selection::All, false, &mut runner),
        ExitCode::FAILURE
    );
}
