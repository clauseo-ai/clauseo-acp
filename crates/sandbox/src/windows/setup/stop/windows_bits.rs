//! The stop's BITS half (`Stopper::bits_jobs`, `Stopper::cancel_job`): every BITS job on the
//! machine, all users' (`BG_JOB_ENUM_ALL_USERS`, which SYSTEM may ask for), with its owner, and
//! `Cancel`, which removes a job. COM's default impersonation level is identify, with which BITS
//! can't act as a job's owner (`Cancel` fails with `ERROR_NOT_LOGGED_ON`), so the manager, the
//! enumerator and each job get the impersonate level before their first call, as the runner's
//! end sweeper does: a proxy reached through another starts with the default again.
#![expect(unsafe_code, reason = "COM calls to BITS")]

use windows::Win32::Networking::BackgroundIntelligentTransferService::{
    BG_JOB_ENUM_ALL_USERS, BackgroundCopyManager, IBackgroundCopyJob, IBackgroundCopyManager,
};
use windows::Win32::System::Com::{
    CLSCTX_LOCAL_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoSetProxyBlanket,
    CoTaskMemFree, CoUninitialize, EOAC_NONE, RPC_C_AUTHN_LEVEL_PKT_PRIVACY,
    RPC_C_IMP_LEVEL_IMPERSONATE,
};
use windows::core::{IUnknown, Interface, PCWSTR};

use super::{Failed, JOBS_CAP};
use crate::windows::sids::Sid;
/// `RPC_C_AUTHN_WINNT`: NTLM or Kerberos, as for any local COM call.
const AUTHN_WINNT: u32 = 10;
/// `RPC_C_AUTHZ_NONE`.
const AUTHZ_NONE: u32 = 0;

/// COM, initialised for this thread while it lives.
pub(super) struct Com(bool);

impl Com {
    pub(super) fn new() -> Com {
        // SAFETY: no reserved pointer; undone on drop when it took.
        Com(unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok())
    }
}

impl Drop for Com {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: balances the CoInitializeEx that took, on the same thread.
            unsafe { CoUninitialize() };
        }
    }
}

/// `call` failed with `error`, as a line for the log.
fn failed(call: &str, error: &windows::core::Error) -> Failed {
    format!("{call} failed ({:#010x})", error.code().0)
}

/// Lets BITS impersonate this process on calls through `proxy`, and returns it.
fn impersonating<T: Interface>(proxy: T) -> Result<T, Failed> {
    let raw = proxy.as_raw();
    // SAFETY: `raw` is `proxy`'s live pointer, borrowed for the call while `proxy` lives.
    let unknown = unsafe { IUnknown::from_raw_borrowed(&raw) }
        .ok_or_else(|| "a null COM proxy".to_owned())?;
    // SAFETY: a live proxy; no server principal and no auth identity: this process's own.
    unsafe {
        CoSetProxyBlanket(
            unknown,
            AUTHN_WINNT,
            AUTHZ_NONE,
            PCWSTR::null(),
            RPC_C_AUTHN_LEVEL_PKT_PRIVACY,
            RPC_C_IMP_LEVEL_IMPERSONATE,
            None,
            EOAC_NONE,
        )
    }
    .map_err(|error| failed("CoSetProxyBlanket", &error))?;
    Ok(proxy)
}

/// Every BITS job, all users', each impersonating this process, with its owner: `None` when it
/// couldn't be read. At most [`JOBS_CAP`]: a list that long may have been cut, which the stop
/// ends at 48 (`cancel_jobs`). COM must be initialised on this thread ([`Com`]) while the jobs
/// live.
pub(super) fn jobs() -> Result<Vec<(IBackgroundCopyJob, Option<Sid>)>, Failed> {
    // SAFETY: a plain out-of-process creation of BITS's manager.
    let manager: IBackgroundCopyManager =
        unsafe { CoCreateInstance(&BackgroundCopyManager, None, CLSCTX_LOCAL_SERVER) }
            .map_err(|error| failed("CoCreateInstance(BackgroundCopyManager)", &error))?;
    let manager = impersonating(manager)?;
    // SAFETY: a live manager; every user's jobs, which SYSTEM may list.
    let list = unsafe { manager.EnumJobs(BG_JOB_ENUM_ALL_USERS) }
        .map_err(|error| failed("IBackgroundCopyManager::EnumJobs", &error))?;
    let list = impersonating(list)?;
    let mut jobs = Vec::new();
    while jobs.len() < JOBS_CAP {
        let mut one: [Option<IBackgroundCopyJob>; 1] = [None];
        let mut fetched = 0u32;
        // SAFETY: a live enumerator; `one` and `fetched` are live out-pointers.
        unsafe { list.Next(&mut one, &raw mut fetched) }
            .map_err(|error| failed("IEnumBackgroundCopyJobs::Next", &error))?;
        let [job] = one;
        let Some(job) = job.filter(|_| fetched == 1) else {
            break;
        };
        let job = impersonating(job)?;
        let owner = owner(&job);
        jobs.push((job, owner));
    }
    Ok(jobs)
}

/// `job`'s owner (`GetOwner`); `None` when it can't be read.
fn owner(job: &IBackgroundCopyJob) -> Option<Sid> {
    // SAFETY: a live job; the string it returns is freed below with CoTaskMemFree.
    let text = unsafe { job.GetOwner() }.ok()?;
    // SAFETY: GetOwner wrote a NUL-terminated string.
    let read = unsafe { text.to_string() };
    // SAFETY: allocated by BITS with CoTaskMemAlloc, freed once.
    unsafe { CoTaskMemFree(Some(text.0.cast_const().cast())) };
    read.ok().and_then(|read| Sid::parse(&read).ok())
}

/// Cancels `job`, which removes it.
pub(super) fn cancel(job: &IBackgroundCopyJob) -> Result<(), Failed> {
    // SAFETY: a live job.
    unsafe { job.Cancel() }.map_err(|error| failed("IBackgroundCopyJob::Cancel", &error))
}
