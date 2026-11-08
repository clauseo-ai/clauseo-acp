//! The admin-only log's writer on a real machine (`windows_sweeper/admin_log.rs`), pointed at a
//! folder of the test's own instead of `%ProgramData%`: a squatted `Clauseo` (made by the
//! test's standard user, or one Users may write) and a `Clauseo\Sandbox` that is a junction are
//! refused, and nothing is written. Each refusal comes before the writer makes anything owned
//! by SYSTEM, so this elevated administrator can run them. Gated like the tests above.

use std::path::Path;

use super::super::windows_sweeper::admin_log::append_in;
use super::{gate, try_in_cmd};
use crate::test_support::TempDir;
use crate::windows::private_dir::set_owner;
use crate::windows::setup::logon::tests::{TestUser, clear_leftovers, random_hex};
use crate::windows::setup::strays::TASK_TEST_PREFIX;
use crate::windows::sids::Sid;
use crate::windows::test_dacl::protect_with;
use crate::windows::test_junction::junction;

/// The installer's DACL for `Clauseo` (`log_path::CLAUSEO_FOLDER_SDDL`), and one that lets
/// Users write too.
const INSTALLER: &str = "D:PAI(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;0x1200a9;;;BU)";
const USERS_WRITE: &str = "D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;0x1301bf;;;BU)";

/// The names in `folder`, for "nothing written".
fn entries(folder: &Path) -> Vec<String> {
    std::fs::read_dir(folder).map_or_else(
        |_| Vec::new(),
        |list| {
            list.map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                .collect()
        },
    )
}

/// Makes the folder `path` as this administrator.
#[expect(
    clippy::disallowed_methods,
    reason = "makes a folder in the test's own temporary folder"
)]
fn make(path: &Path) {
    std::fs::create_dir(path).unwrap();
}

#[test]
#[ignore = "elevated: run by CI's elevated step with CLAUSEO_ELEVATED_TESTS=1"]
fn elevated_the_admin_log_writer_refuses_a_squatted_clauseo_folder_and_writes_nothing() {
    if !gate("elevated_the_admin_log_writer_refuses_a_squatted_clauseo_folder_and_writes_nothing") {
        return;
    }
    // (a1) Made by the test's own standard user, in a folder it may write: its owner.
    clear_leftovers(TASK_TEST_PREFIX);
    let user = TestUser::named(TASK_TEST_PREFIX, &random_hex(3));
    let base = TempDir::new("admin-log-squat");
    let grant = format!(
        "D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FA;;;{})",
        user.sid.as_str()
    );
    protect_with(base.path(), &grant).unwrap();
    let clauseo = base.path().join("Clauseo");
    let (code, printed) = try_in_cmd(
        &user,
        base.path(),
        &format!(r#"mkdir "{}""#, clauseo.display()),
    );
    assert_eq!(code, Some(0), "mkdir as the test user:\n{printed}");
    let refused = append_in(base.path(), "a line");
    eprintln!("(a1) user-owned: {refused:?}");
    let why = refused.expect_err("a user-owned Clauseo was accepted");
    assert!(why.contains("change it"), "{why}");
    assert_eq!(entries(&clauseo), Vec::<String>::new(), "nothing written");

    // (a2) Made by this administrator, owned by Administrators, but Users may write it.
    let base = TempDir::new("admin-log-writable");
    let clauseo = base.path().join("Clauseo");
    make(&clauseo);
    set_owner(&clauseo, &Sid::parse("S-1-5-32-544").unwrap()).unwrap();
    protect_with(&clauseo, USERS_WRITE).unwrap();
    let refused = append_in(base.path(), "a line");
    eprintln!("(a2) Users may write: {refused:?}");
    let why = refused.expect_err("a Clauseo Users may write was accepted");
    assert!(why.contains("change it"), "{why}");
    assert_eq!(entries(&clauseo), Vec::<String>::new(), "nothing written");
}

#[test]
#[ignore = "elevated: run by CI's elevated step with CLAUSEO_ELEVATED_TESTS=1"]
fn elevated_the_admin_log_writer_refuses_a_sandbox_folder_that_is_a_junction() {
    if !gate("elevated_the_admin_log_writer_refuses_a_sandbox_folder_that_is_a_junction") {
        return;
    }
    // (b) An installer-shaped Clauseo, its Sandbox a junction to another folder.
    let base = TempDir::new("admin-log-junction");
    let clauseo = base.path().join("Clauseo");
    make(&clauseo);
    set_owner(&clauseo, &Sid::parse("S-1-5-32-544").unwrap()).unwrap();
    protect_with(&clauseo, INSTALLER).unwrap();
    let elsewhere = base.path().join("elsewhere");
    make(&elsewhere);
    junction(&clauseo.join("Sandbox"), &elsewhere).unwrap();
    let refused = append_in(base.path(), "a line");
    eprintln!("(b) junction: {refused:?}");
    let why = refused.expect_err("a junction at Sandbox was followed");
    assert!(why.contains("reparse point"), "{why}");
    assert_eq!(
        entries(&elsewhere),
        Vec::<String>::new(),
        "nothing written through it"
    );
}
