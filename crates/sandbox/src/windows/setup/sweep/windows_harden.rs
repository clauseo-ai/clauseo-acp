//! Hardening on Windows (`harden.rs`): a task's descriptor read and written through the task's
//! own interface (`IRegisteredTask::GetSecurityDescriptor` and `SetSecurityDescriptor`, whose
//! SDDL is converted to and from the self-relative bytes `harden.rs` works on), and set-up's
//! call ([`at_set_up`]). Whether a task needs hardening is read without its SACL, which needs
//! no privilege; only a task that does is read whole, SACL included, and that whole descriptor
//! is written back with only its DACL changed, so the owner, group and SACL stay as read
//! (`harden.rs` reads it back to check). `SeSecurityPrivilege` is enabled for the SACL's
//! reads and the write alone. The write passes
//! `TASK_DONT_ADD_PRINCIPAL_ACE`: without it the Task Scheduler adds an allow for the task's
//! principal, and hardening never adds a right.
#![expect(
    unsafe_code,
    reason = "COM calls to a task's descriptor, and the SDDL conversions around them"
)]

use windows::Win32::Foundation::{HLOCAL, LocalFree};
use windows::Win32::Security::Authorization::{
    ConvertSecurityDescriptorToStringSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{
    DACL_SECURITY_INFORMATION, GROUP_SECURITY_INFORMATION, GetSecurityDescriptorLength,
    OBJECT_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
    SACL_SECURITY_INFORMATION,
};
use windows::Win32::System::TaskScheduler::{ITaskFolder, TASK_DONT_ADD_PRINCIPAL_ACE};
use windows::core::{BSTR, PWSTR};

use super::harden::{TaskSecurity, harden_loose_tasks, to_admin_log};
use super::windows_sweeper::{WindowsSweeper, failed};
use super::{Failed, untrusted};
use crate::windows::acl::privilege::Privileges;
use crate::windows::setup::lock::Descriptor;
use crate::windows::setup::pool_steps::windows_gate;

/// The parts read and written: owner, group, DACL and SACL.
const PARTS: OBJECT_SECURITY_INFORMATION = OBJECT_SECURITY_INFORMATION(
    OWNER_SECURITY_INFORMATION.0
        | GROUP_SECURITY_INFORMATION.0
        | DACL_SECURITY_INFORMATION.0
        | SACL_SECURITY_INFORMATION.0,
);

/// What reading or writing a SACL needs.
const SECURITY: [&str; 1] = ["SeSecurityPrivilege"];

/// The parts read to tell whether a task needs hardening: owner, group and DACL, which need no
/// privilege.
const WITHOUT_SACL: OBJECT_SECURITY_INFORMATION = OBJECT_SECURITY_INFORMATION(
    OWNER_SECURITY_INFORMATION.0 | GROUP_SECURITY_INFORMATION.0 | DACL_SECURITY_INFORMATION.0,
);

/// The descriptor of the task `name` in `folder`, as `Sweeper::task_security` says: with the
/// SACL (`SeSecurityPrivilege` enabled for the call) when `sacl`.
pub(super) fn task_security(
    folder: &ITaskFolder,
    name: &str,
    sacl: bool,
) -> Result<TaskSecurity, Failed> {
    // SAFETY: a live folder and a BSTR that outlives the call.
    let task =
        unsafe { folder.GetTask(&BSTR::from(name)) }.map_err(failed("ITaskFolder::GetTask"))?;
    let parts = if sacl { PARTS } else { WITHOUT_SACL };
    let parts = i32::try_from(parts.0).unwrap_or(i32::MAX);
    let sddl = {
        let _security = if sacl {
            Some(Privileges::enable(&SECURITY).map_err(|error| error.to_string())?)
        } else {
            None
        };
        // SAFETY: a live task; the parts asked for.
        unsafe { task.GetSecurityDescriptor(parts) }
            .map_err(failed("IRegisteredTask::GetSecurityDescriptor"))?
            .to_string()
    };
    let descriptor = Descriptor::from_sddl(&sddl).map_err(|error| error.to_string())?;
    // SAFETY: the conversion made a valid self-relative descriptor, alive until it drops.
    let size = unsafe { GetSecurityDescriptorLength(descriptor.0) };
    let size = usize::try_from(size).map_err(|error| error.to_string())?;
    // SAFETY: the descriptor is `size` bytes long and alive for the copy.
    let bytes = unsafe { std::slice::from_raw_parts(descriptor.0.0.cast::<u8>(), size) };
    Ok(TaskSecurity {
        bytes: bytes.to_vec(),
        sddl,
    })
}

/// Gives the task `name` in `folder` the descriptor `descriptor`, as
/// `Sweeper::set_task_security` says.
pub(super) fn set_task_security(
    folder: &ITaskFolder,
    name: &str,
    descriptor: &[u8],
) -> Result<(), Failed> {
    // u64 words, so the descriptor is aligned for the conversion.
    let mut words = vec![0_u64; descriptor.len().div_ceil(8).max(1)];
    for (word, chunk) in words.iter_mut().zip(descriptor.chunks(8)) {
        let mut bytes = [0_u8; 8];
        for (slot, byte) in bytes.iter_mut().zip(chunk) {
            *slot = *byte;
        }
        *word = u64::from_ne_bytes(bytes);
    }
    let mut text = PWSTR::null();
    // SAFETY: `words` holds the self-relative descriptor and outlives the call; `text`
    // receives a LocalAlloc'd string, freed below.
    unsafe {
        ConvertSecurityDescriptorToStringSecurityDescriptorW(
            PSECURITY_DESCRIPTOR(words.as_mut_ptr().cast()),
            SDDL_REVISION_1,
            PARTS,
            &raw mut text,
            None,
        )
    }
    .map_err(failed(
        "ConvertSecurityDescriptorToStringSecurityDescriptorW",
    ))?;
    // SAFETY: on success the call wrote a NUL-terminated string at `text`.
    let sddl = unsafe { text.to_string() }.unwrap_or_default();
    // SAFETY: `text` came from the conversion's LocalAlloc and is freed once, here.
    unsafe { LocalFree(Some(HLOCAL(text.0.cast()))) };
    // SAFETY: a live folder and a BSTR that outlives the call.
    let task =
        unsafe { folder.GetTask(&BSTR::from(name)) }.map_err(failed("ITaskFolder::GetTask"))?;
    let _security = Privileges::enable(&SECURITY).map_err(|error| error.to_string())?;
    // SAFETY: a live task and a BSTR that outlives the call; no entry added for the principal.
    unsafe { task.SetSecurityDescriptor(&BSTR::from(sddl), TASK_DONT_ADD_PRINCIPAL_ACE.0) }
        .map_err(failed("IRegisteredTask::SetSecurityDescriptor"))
}

/// Set-up's hardening (`late_steps::harden_tasks`), as its elevated administrator: the pool
/// the record names (just written by this set-up), every task that runs as the lawyer hardened
/// ([`harden_loose_tasks`]), and what changed appended to the admin-only log. Set-up makes that
/// log when it is missing, owned by Administrators (an administrator can't make a file owned by
/// SYSTEM; `admin_log::append_as_admin`), and the sweep's checks take either owner. A task
/// whose `Id` changed across the write goes to the record's `OwnerUntrusted`. Returns
/// the counts for the Event Log, `""` when there was nothing to do; `Err` when the record
/// can't be read.
pub(crate) fn at_set_up() -> Result<String, Failed> {
    let Some(pool) = windows_gate::pool()? else {
        return Ok(String::new());
    };
    let mut sweeper = WindowsSweeper::for_set_up();
    let hardened = harden_loose_tasks(&mut sweeper, &pool);
    let mut line = to_admin_log(&mut sweeper, &hardened, "set-up");
    if hardened.unlisted > 0 {
        let unread = hardened.unlisted;
        line = format!("{line}; {unread} task folders couldn't be read, their tasks unchecked");
    }
    if !hardened.swapped_after.is_empty() {
        // Set-up holds the set-up lock already.
        let stored = match untrusted::read(&mut sweeper) {
            Ok(read) => {
                let list =
                    untrusted::with_added(&mut sweeper, read.clone(), &hardened.swapped_after);
                untrusted::store(&mut sweeper, &read, &list, false)
            }
            Err(why) => Err(why),
        };
        if let Err(why) = stored {
            line = format!("{line}; the record's OwnerUntrusted couldn't be written: {why}");
        }
    }
    Ok(line)
}
