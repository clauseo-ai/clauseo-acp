//! The owner's cleanup: undoes what the row made, in a fixed order, each step checked and
//! printed. It runs after the row and, if the row panicked, from `Drop for Run`. The removals
//! that could pass unnoticed are confirmed: the account's processes are gone before anything
//! they hold is removed, the namespace no longer opens by name, the station no longer opens by
//! name, and the test root no longer exists.

use std::io::ErrorKind;
use std::path::Path;

use clauseo_lowbox::{Error, NamespaceHandles};
use windows::Win32::Foundation::STATUS_OBJECT_NAME_NOT_FOUND;

use super::evidence::redact;
use super::owner::{Built, Run};
use super::win::{Res, sleep_ms};
use super::{account, loopback, write};

/// How many times the test root's delete is tried (10 s at [`ROOT_PAUSE_MS`]).
const ROOT_TRIES: u32 = 20;
/// The pause between those tries.
const ROOT_PAUSE_MS: u32 = 500;

/// Undoes what `run` made, in the brief's order, printing each step; the failures.
pub(crate) fn cleanup(run: &mut Run) -> Vec<String> {
    let mut failures = Vec::new();
    let mut step = |name: &str, result: Res<String>| match result {
        Ok(done) => println!("cleanup: {name}: {done}"),
        Err(e) => {
            println!("cleanup: {name}: FAILED: {e}");
            failures.push(format!("{name}: {e}"));
        }
    };
    if let Some(helper) = run.helper.take() {
        step("kill leftovers", helper.kill());
    }
    if let Some(sid) = run.user.as_ref().and_then(|u| u.sid.as_deref()) {
        step(
            "wait for the account's processes to end",
            account::wait_processes_gone(sid),
        );
    }
    if let Some(mut proxy) = run.proxy.take() {
        step("stop the proxy", Ok(proxy.stop()));
    }
    if let Some(built) = run.namespace.take() {
        step("delete the namespace", namespace_gone(built));
    }
    if let Some(drive_root) = run.drive_root.take() {
        step("restore the drive root's DACL", drive_root.restore());
    }
    if let Some(null_device) = run.null_device.take() {
        step(r"restore \Device\Null's DACL", null_device.restore());
    }
    if let Some(original) = run.loopback.take() {
        step("restore the loopback list", loopback::restore(&original));
    }
    if let Some(sid) = run.logon_right.take() {
        step("remove the account's rights", account::remove_logon(&sid));
    }
    if let Some(mut station) = run.station.take() {
        step("close the station and desktop", station.close());
    }
    if let Some(user) = run.user.take() {
        step("NetUserDel", account::delete(&user.name));
        if let Some(sid) = user.sid {
            step(
                "DeleteProfileW",
                account::delete_profile(&sid, &user.profile_fallback),
            );
        }
    }
    if let Some(root) = run.root.take() {
        step("delete the test root", delete_root(&root));
    }
    run.cleaned = true;
    failures
}

impl Drop for Run {
    fn drop(&mut self) {
        if !self.cleaned {
            println!("cleanup after a panic:");
            let failures = cleanup(self);
            println!("cleanup after a panic: {} step(s) failed", failures.len());
        }
    }
}

/// Closes the owner's six namespace handles, then checks the package directory no longer opens
/// by name: `NamespaceHandles::open` must fail with `STATUS_OBJECT_NAME_NOT_FOUND`.
fn namespace_gone(built: Built) -> Res<String> {
    let Built {
        handles,
        package,
        session,
    } = built;
    drop(handles);
    match NamespaceHandles::open(&package, session) {
        Err(Error::Nt { status, .. }) if status == STATUS_OBJECT_NAME_NOT_FOUND.0 => Ok(
            "closed all six; the package directory no longer opens by name \
                (STATUS_OBJECT_NAME_NOT_FOUND)"
                .to_owned(),
        ),
        Err(e) => Err(redact(&format!(
            "closed all six, but opening the namespace by name failed otherwise: {e}"
        ))),
        Ok(again) => {
            drop(again);
            Err("closed all six, but the namespace still opens by name".to_owned())
        }
    }
}

/// Deletes the test root, retrying while the last of chrome's files close, then checks it no
/// longer exists.
fn delete_root(root: &Path) -> Res<String> {
    let mut last = String::new();
    for attempt in 1..=ROOT_TRIES {
        match write::remove_dir_all(root) {
            Ok(()) => {}
            Err(e) if e.kind() == ErrorKind::NotFound => {}
            Err(e) => last = e.to_string(),
        }
        match root.try_exists() {
            Ok(false) => {
                return Ok(format!(
                    "deleted on try {attempt}; {} no longer exists",
                    root.display()
                ));
            }
            Ok(true) if last.is_empty() => last = "it still exists".to_owned(),
            Ok(true) => {}
            Err(e) => last = format!("checking it: {e}"),
        }
        if attempt < ROOT_TRIES {
            sleep_ms(ROOT_PAUSE_MS);
        }
    }
    Err(format!(
        "{} remains after {ROOT_TRIES} tries: {last}",
        root.display()
    ))
}
