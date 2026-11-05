//! The built app under a restricted token (Windows CI): started with a token from
//! `CreateRestrictedToken` that has restricting SIDs, it refuses with exit 77 and exactly its one
//! line; the pre-write guard still runs under such a token, and under one without restricting
//! SIDs. The restricting SIDs are the process's own user and groups, so every access check passes
//! as before (the exe, its DLLs, the window station) and only the restriction is under test. A
//! start that wrongly reached the app would open a window and hang until nextest's timeout ends
//! the test, which fails it. S15's sandbox user and an `AppContainer` need set-up a runner
//! cannot undo cleanly; they are a VM row.
#![cfg(windows)]
#![cfg_attr(
    windows,
    expect(
        unsafe_code,
        reason = "CreateRestrictedToken and CreateProcessAsUserW, which std has no binding for"
    )
)]

use std::ffi::OsStr;
use std::io::{Read as _, Write as _};
use std::os::windows::ffi::OsStrExt as _;
use std::os::windows::io::AsRawHandle as _;

use clauseo_desktop::{REFUSAL, SANDBOXED_EXIT};
use windows::Win32::Foundation::{
    CloseHandle, HANDLE, HANDLE_FLAG_INHERIT, SetHandleInformation, WAIT_OBJECT_0,
};
use windows::Win32::Security::{
    CREATE_RESTRICTED_TOKEN_FLAGS, CreateRestrictedToken, DISABLE_MAX_PRIVILEGE,
    GetTokenInformation, SID_AND_ATTRIBUTES, TOKEN_ASSIGN_PRIMARY, TOKEN_DUPLICATE, TOKEN_GROUPS,
    TOKEN_INFORMATION_CLASS, TOKEN_QUERY, TOKEN_USER, TokenGroups, TokenUser, WRITE_RESTRICTED,
};
use windows::Win32::System::Threading::{
    CREATE_NO_WINDOW, CreateProcessAsUserW, GetCurrentProcess, GetExitCodeProcess, INFINITE,
    OpenProcessToken, PROCESS_INFORMATION, STARTF_USESTDHANDLES, STARTUPINFOW, WaitForSingleObject,
};
use windows::core::{PCWSTR, PWSTR};

/// The binary under test.
const APP: &str = env!("CARGO_BIN_EXE_clauseo-desktop");

/// `SE_GROUP_INTEGRITY`: the token's integrity label, which is no group to restrict to.
const SE_GROUP_INTEGRITY: u32 = 0x20;

/// A handle, closed on drop.
struct Handle(HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: the handle is open and closed once, here.
        drop(unsafe { CloseHandle(self.0) });
    }
}

/// This process's primary token, opened to query, duplicate and assign.
#[cfg(test)]
fn own_token() -> Handle {
    let mut handle = HANDLE::default();
    let access = TOKEN_QUERY | TOKEN_DUPLICATE | TOKEN_ASSIGN_PRIMARY;
    // SAFETY: GetCurrentProcess is a pseudo-handle; `handle` is a live out-pointer.
    unsafe { OpenProcessToken(GetCurrentProcess(), access, &raw mut handle) }.unwrap();
    Handle(handle)
}

/// The token's `class` information, in u64 words (aligned for the pointers it holds).
#[cfg(test)]
fn information(token: &Handle, class: TOKEN_INFORMATION_CLASS) -> Vec<u64> {
    let mut needed = 0u32;
    // SAFETY: no buffer asks only for the size; the expected "buffer too small" is dropped.
    drop(unsafe { GetTokenInformation(token.0, class, None, 0, &raw mut needed) });
    let mut buffer = vec![0u64; usize::try_from(needed).unwrap().div_ceil(8)];
    let len = u32::try_from(buffer.len() * 8).unwrap();
    // SAFETY: `buffer` is live, writable and `len` bytes long.
    unsafe {
        GetTokenInformation(
            token.0,
            class,
            Some(buffer.as_mut_ptr().cast()),
            len,
            &raw mut needed,
        )
    }
    .unwrap();
    buffer
}

/// A restricted copy of this process's token made with `flags`; with `restrict`, its user and
/// every group (but the integrity label) are its restricting SIDs.
#[cfg(test)]
fn restricted_token(flags: CREATE_RESTRICTED_TOKEN_FLAGS, restrict: bool) -> Handle {
    let token = own_token();
    let user_buffer = information(&token, TokenUser);
    let groups_buffer = information(&token, TokenGroups);
    // SAFETY: GetTokenInformation wrote a TOKEN_USER at the start of the aligned buffer, which
    // outlives every use of it below.
    let user = unsafe { &*user_buffer.as_ptr().cast::<TOKEN_USER>() };
    let groups = groups_buffer.as_ptr().cast::<TOKEN_GROUPS>();
    // SAFETY: a TOKEN_GROUPS starts the aligned buffer; its Groups array holds GroupCount
    // entries inside the buffer, which outlives the slice.
    let groups = unsafe {
        std::slice::from_raw_parts(
            (&raw const (*groups).Groups).cast::<SID_AND_ATTRIBUTES>(),
            usize::try_from((*groups).GroupCount).unwrap(),
        )
    };
    let sids: Vec<SID_AND_ATTRIBUTES> = std::iter::once(user.User.Sid)
        .chain(
            groups
                .iter()
                .filter(|group| group.Attributes & SE_GROUP_INTEGRITY == 0)
                .map(|group| group.Sid),
        )
        .map(|sid| SID_AND_ATTRIBUTES {
            Sid: sid,
            Attributes: 0,
        })
        .collect();
    let mut new = HANDLE::default();
    let restricting = restrict.then_some(sids.as_slice());
    // SAFETY: `token` is open with TOKEN_DUPLICATE; the SIDs point into the live buffers above;
    // `new` is a live out-pointer.
    unsafe { CreateRestrictedToken(token.0, flags, None, None, restricting, &raw mut new) }
        .unwrap();
    Handle(new)
}

/// Makes `handle` inheritable by the child.
#[cfg(test)]
fn inheritable(handle: HANDLE) {
    // SAFETY: the handle is open for the call.
    unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT.0, HANDLE_FLAG_INHERIT) }.unwrap();
}

/// NUL-terminated UTF-16.
#[cfg(test)]
fn wide(text: &str) -> Vec<u16> {
    OsStr::new(text).encode_wide().chain([0]).collect()
}

/// Starts the app with `args` under `token`, `input` on its stdin; returns its exit code and
/// what it wrote to stdout and stderr (one pipe for both, so the text shows stdout stayed
/// empty).
#[cfg(test)]
fn start(token: &Handle, args: &[&str], input: &[u8]) -> (u32, String) {
    let (child_in, mut feed) = std::io::pipe().unwrap();
    let (mut output, child_out) = std::io::pipe().unwrap();
    // Small enough to sit in the pipe's buffer before the child reads it.
    feed.write_all(input).unwrap();
    drop(feed);
    let stdin = HANDLE(child_in.as_raw_handle());
    let stdout = HANDLE(child_out.as_raw_handle());
    inheritable(stdin);
    inheritable(stdout);
    let app = wide(APP);
    let mut line = wide(&format!("\"{APP}\" {}", args.join(" ")));
    // SAFETY: STARTUPINFOW is plain data; all zeros is its empty value.
    let mut startup: STARTUPINFOW = unsafe { std::mem::zeroed() };
    startup.cb = u32::try_from(size_of::<STARTUPINFOW>()).unwrap();
    startup.dwFlags = STARTF_USESTDHANDLES;
    startup.hStdInput = stdin;
    startup.hStdOutput = stdout;
    startup.hStdError = stdout;
    let mut process = PROCESS_INFORMATION::default();
    // SAFETY: `token` is a primary token restricted from this process's own, so no privilege is
    // needed to assign it; `app` and `line` are live NUL-terminated strings (`line` writable);
    // `startup` and `process` live for the call.
    unsafe {
        CreateProcessAsUserW(
            Some(token.0),
            PCWSTR(app.as_ptr()),
            Some(PWSTR(line.as_mut_ptr())),
            None,
            None,
            true,
            CREATE_NO_WINDOW,
            None,
            PCWSTR::null(),
            &raw const startup,
            &raw mut process,
        )
    }
    .unwrap();
    let _thread = Handle(process.hThread);
    let process = Handle(process.hProcess);
    // Only the child holds the pipe ends now, so the read ends when it exits.
    drop(child_in);
    drop(child_out);
    let mut text = String::new();
    output.read_to_string(&mut text).unwrap();
    // SAFETY: the process handle is open.
    let waited = unsafe { WaitForSingleObject(process.0, INFINITE) };
    assert_eq!(waited, WAIT_OBJECT_0);
    let mut code = 0u32;
    // SAFETY: the process handle is open; `code` is a live out-pointer.
    unsafe { GetExitCodeProcess(process.0, &raw mut code) }.unwrap();
    (code, text)
}

/// A hook input under Codex's hook (no `--matter`): a `Bash` command naming `.codex`, which the
/// guard refuses with its code 2.
#[cfg(test)]
fn codex_write() -> Vec<u8> {
    let cwd = std::env::current_dir().unwrap();
    serde_json::to_vec(&serde_json::json!({
        "cwd": cwd,
        "tool_name": "Bash",
        "tool_input": { "command": "touch .codex/config.toml" },
    }))
    .unwrap()
}

#[cfg(test)]
fn assert_refused((code, text): (u32, String)) {
    assert_eq!(code, u32::try_from(SANDBOXED_EXIT).unwrap(), "{text}");
    assert_eq!(text, format!("{REFUSAL}\n"));
}

#[cfg(test)]
fn assert_guard_refused((code, text): (u32, String)) {
    assert_eq!(code, 2, "{text}");
    assert!(!text.is_empty() && !text.contains(REFUSAL), "{text}");
}

#[test]
fn under_restricting_sids_the_app_refuses() {
    let token = restricted_token(CREATE_RESTRICTED_TOKEN_FLAGS(0), true);
    assert_refused(start(&token, &[], b""));
    // A deep link, which a second start would hand to the running window.
    assert_refused(start(&token, &["clauseo://open"], b""));
}

#[test]
fn under_a_write_restricted_token_the_app_refuses() {
    let token = restricted_token(WRITE_RESTRICTED, true);
    assert_refused(start(&token, &[], b""));
}

#[test]
fn the_guard_runs_under_restricting_sids() {
    let token = restricted_token(DISABLE_MAX_PRIVILEGE, true);
    assert_guard_refused(start(&token, &["--pre-write-guard"], &codex_write()));
}

#[test]
fn the_guard_runs_under_a_token_without_restricting_sids() {
    let token = restricted_token(DISABLE_MAX_PRIVILEGE, false);
    assert_guard_refused(start(&token, &["--pre-write-guard"], &codex_write()));
}
