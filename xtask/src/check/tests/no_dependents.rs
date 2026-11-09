//! `--no-dependents` (`quick_scope` with `no_dependents`): the changed packages only, and the
//! summary line says the dependents are skipped.

use super::*;

#[test]
fn no_dependents_selects_the_changed_package_and_says_its_dependents_are_skipped() {
    let (_root, mut ctx) = scratch_context("check-quick-no-dependents");
    ctx.members = Ok(core_and_app());
    let mut out = Printer::new(Vec::<u8>::new(), false);
    let scope = quick_scope(
        &mut out,
        &ctx,
        Ok(vec!["crates/core/src/lib.rs".into()]),
        true,
    );
    assert_eq!(
        printed(out),
        "quick: packages core (changed: core; dependents skipped: --no-dependents)\n"
    );
    assert_eq!(scope, Scope::packages(vec!["core".into()]));
}

#[test]
fn no_dependents_keeps_the_test_binary_a_changed_package_needs() {
    let (_root, mut ctx) = scratch_context("check-quick-no-dependents-fake-agent");
    ctx.members = Ok(core_app_and_fake_agent());
    let mut out = Printer::new(Vec::<u8>::new(), false);
    let scope = quick_scope(
        &mut out,
        &ctx,
        Ok(vec!["crates/app/src/main.rs".into()]),
        true,
    );
    assert_eq!(
        printed(out),
        "quick: packages app, clauseo-fake_agent (changed: app; dependents skipped: \
         --no-dependents; built for their tests: clauseo-fake_agent)\n"
    );
    assert_eq!(
        scope,
        Scope::packages(vec!["app".into(), "clauseo-fake_agent".into()])
    );
}

#[test]
fn no_dependents_with_no_changed_package_selects_nothing() {
    let (_root, mut ctx) = scratch_context("check-quick-no-dependents-none");
    ctx.members = Ok(core_and_app());
    let mut out = Printer::new(Vec::<u8>::new(), false);
    let scope = quick_scope(&mut out, &ctx, Ok(vec!["README.md".into()]), true);
    assert_eq!(printed(out), "quick: no changed packages\n");
    assert_eq!(scope, Scope::Nothing);
}

#[test]
fn no_dependents_still_selects_every_package_when_a_root_file_changes() {
    let (_root, mut ctx) = scratch_context("check-quick-no-dependents-all");
    ctx.members = Ok(core_and_app());
    let mut out = Printer::new(Vec::<u8>::new(), false);
    let scope = quick_scope(&mut out, &ctx, Ok(vec!["Cargo.lock".into()]), true);
    assert_eq!(printed(out), "quick: all packages (Cargo.lock changed)\n");
    assert_eq!(scope, Scope::Workspace);
}

#[test]
fn no_dependents_narrows_quick_and_changes_nothing_without_it() {
    assert_eq!(Quick::from_flags(false, false), Quick::Off);
    assert_eq!(Quick::from_flags(false, true), Quick::Off);
    assert_eq!(Quick::from_flags(true, false), Quick::WithDependents);
    assert_eq!(Quick::from_flags(true, true), Quick::ChangedOnly);
}

/// `main.rs` rejects `--no-dependents` without `--quick`; given it anyway, `run` lints the
/// workspace.
#[test]
fn no_dependents_without_quick_lints_the_workspace() {
    let (_root, ctx) = scratch_context("check-run-no-dependents");
    let ran = RefCell::new(Vec::new());
    let mut exec = |cmd: &Cmd, _: &mut dyn FnMut(&[u8])| Ok(record_and_fail(&ran, cmd));
    let mut quiet = |_: &Cmd, _: &Path| -> io::Result<Ran> { unreachable!("CI does not probe") };
    let flags = Flags {
        quick: Quick::from_flags(false, true),
        ..Flags::default()
    };
    run(&ctx, flags, true, &mut exec, &mut quiet);
    assert!(
        ran.borrow()
            .iter()
            .any(|cmd| cmd.starts_with("cargo clippy --workspace")),
        "{:?}",
        ran.borrow()
    );
}
