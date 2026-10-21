//! Tests for `install.rs`'s winget path when winget exits 0 (the failing cases are in `tests.rs`).

use super::*;

#[test]
fn a_winget_that_exits_zero_installs_and_prints_only_its_command() {
    let lefthook = binary("lefthook");
    let winget = expected_winget(lefthook);
    let machine = Machine {
        os: "windows",
        ..linux(Path::new("unused"), true)
    };
    let (ok, ran, printed) = install_with(lefthook, &machine, b"", &mut |_| exited(true, "", ""));
    assert!(ok);
    assert_eq!(ran, [(winget.clone(), PathBuf::from("."))]);
    assert_eq!(
        printed,
        format!("installing lefthook {}: {winget}\n", lefthook.version)
    );
}
