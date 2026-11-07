//! The per-user class removal on a real machine (N3): an in-process server planted for the Task
//! Scheduler's class, in both registry views, is gone before COM starts. The keys are planted
//! under a scratch root of this user's own `HKEY_CURRENT_USER`, never its real classes. And the
//! BITS calls let BITS impersonate this user, on the manager and on each job listed, before
//! `EnumJobs` and `Cancel`: one job of this user's, made here, is found and cancelled.
#![expect(unsafe_code, reason = "FFI to the registry, COM and BITS in tests")]

use windows::Win32::Networking::BackgroundIntelligentTransferService::BG_JOB_TYPE_DOWNLOAD;
use windows::Win32::System::Com::{CoQueryProxyBlanket, RPC_C_IMP_LEVEL_IMPERSONATE};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey,
    RegCreateKeyExW, RegSetValueExW,
};
use windows::core::{GUID, IUnknown, Interface, PCWSTR};

use super::super::{Kind, remove_all};
use super::*;
use crate::windows::token::os::{own_token, user};

/// The Task Scheduler's class, which the end sweeper makes in-process.
const TASK_SCHEDULER: &str = "{0F87369F-A4E5-4CFC-BD3E-73E6154572DD}";

/// Makes `HKEY_CURRENT_USER\<path>` with `dll` as its default value.
fn plant(path: &str, dll: &str) {
    let wide_path = wide(path);
    let mut key = HKEY::default();
    // SAFETY: the path is NUL-terminated; `key` is a live out-pointer, closed below.
    let code = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(wide_path.as_ptr()),
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            None,
            &raw mut key,
            None,
        )
    };
    assert_eq!(code, ERROR_SUCCESS, "{path}");
    let value: Vec<u8> = wide(dll)
        .iter()
        .flat_map(|unit| unit.to_le_bytes())
        .collect();
    // SAFETY: the key is open with KEY_WRITE; `value` is a NUL-terminated UTF-16 string.
    let set = unsafe { RegSetValueExW(key, PCWSTR::null(), None, REG_SZ, Some(&value)) };
    // SAFETY: opened above, closed once.
    let _closed = unsafe { RegCloseKey(key) };
    assert_eq!(set, ERROR_SUCCESS);
}

fn exists(path: &str) -> bool {
    let wide_path = wide(path);
    open_key(PCWSTR(wide_path.as_ptr()), KEY_QUERY_VALUE)
        .unwrap()
        .is_some()
}

#[test]
fn a_planted_task_scheduler_server_is_removed_before_com() {
    let mut random = [0u8; 8];
    getrandom::fill(&mut random).unwrap();
    let scratch = format!(
        "Software\\ClauseoTest-{}",
        crate::windows::pipe::hex(&random)
    );
    let classes = format!("{scratch}\\Classes");
    let planted = [
        format!("{classes}\\CLSID\\{TASK_SCHEDULER}\\InprocServer32"),
        format!("{classes}\\Wow6432Node\\CLSID\\{TASK_SCHEDULER}\\InprocServer32"),
        format!("{classes}\\Interface\\{TASK_SCHEDULER}"),
    ];
    for path in &planted {
        plant(path, r"C:\Users\Public\evil.dll");
    }
    let token = own_token().unwrap();
    let me = user(&token).unwrap();
    let mut calls = OsLeftovers::new(&token, me);
    calls.classes.clone_from(&classes);
    let mut lines = Vec::new();
    let clean = remove_all(&mut calls, &[Kind::Classes], "test", &mut |line| {
        lines.push(line.to_owned());
    });
    let left: Vec<&String> = planted.iter().filter(|path| exists(path)).collect();
    let com_started = calls.com.is_some();
    drop(calls);
    // The scratch root goes whatever the result.
    drop(delete_tree(&wide(&scratch)));
    assert!(clean, "{lines:?}");
    assert!(left.is_empty(), "left: {left:?}");
    assert_eq!(
        lines,
        ["clauseo-sandbox-run: cleanup (test): removed 3 per-user COM class keys"]
    );
    assert!(!com_started, "the classes alone never start COM");
    assert!(!exists(&format!("{classes}\\CLSID")));
}

/// The impersonation level set on `proxy`.
fn impersonation<T: Interface>(proxy: &T) -> u32 {
    let raw = proxy.as_raw();
    // SAFETY: `raw` is `proxy`'s live pointer, borrowed for the call.
    let unknown = unsafe { IUnknown::from_raw_borrowed(&raw) }.unwrap();
    let mut level = 0u32;
    // SAFETY: a live proxy; `level` is a live out-pointer, the rest aren't asked for.
    unsafe {
        CoQueryProxyBlanket(
            unknown,
            None,
            None,
            None,
            None,
            Some(&raw mut level),
            None,
            None,
        )
    }
    .unwrap();
    level
}

/// A job made by the test, cancelled on drop whatever the result.
struct Made(IBackgroundCopyJob);

impl Drop for Made {
    fn drop(&mut self) {
        // SAFETY: a live job; one cancelled already just fails.
        drop(unsafe { self.0.Cancel() });
    }
}

#[test]
fn the_bits_calls_let_bits_impersonate_before_enum_jobs_and_cancel() {
    let _com = com::Com::new();
    let manager = com::manager().unwrap();
    assert_eq!(impersonation(&manager), RPC_C_IMP_LEVEL_IMPERSONATE.0);
    let mut id = GUID::zeroed();
    let mut made = None;
    // SAFETY: a live manager; `id` and `made` are live out-pointers.
    unsafe {
        manager.CreateJob(
            windows::core::w!("ClauseoTest blanket"),
            BG_JOB_TYPE_DOWNLOAD,
            &raw mut id,
            &raw mut made,
        )
    }
    .unwrap();
    let _made = Made(made.unwrap());
    let items = com::bits_jobs().unwrap();
    let job = items
        .iter()
        .find_map(|item| match item {
            // SAFETY: a live job.
            Item::Bits(job) if unsafe { job.GetId() }.ok() == Some(id) => Some(job),
            Item::Bits(_)
            | Item::Class(_)
            | Item::Task { .. }
            | Item::Value { .. }
            | Item::Entry { .. } => None,
        })
        .expect("the job made is listed");
    assert_eq!(impersonation(job), RPC_C_IMP_LEVEL_IMPERSONATE.0);
    com::cancel_job(job).unwrap();
}
