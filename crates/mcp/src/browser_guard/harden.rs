//! The guard's own process, hidden from other processes of the user before any secret exists
//! (route 2, decision record 0060).
//!
//! Linux: [`not_dumpable`] sets `PR_SET_DUMPABLE` to 0, which makes `/proc/<guard>/` root's, so
//! a same-user process can read neither the guard's `environ` and `mem` nor its fds, and cannot
//! attach to it. A child's exec makes it dumpable again, so agent-browser's environment (the
//! relay's URL) stays readable: the relay's peer check (`devtools/peer.rs`) is what keeps other
//! processes out. macOS: nothing to set. `task_for_pid` on another process needs an entitlement
//! or root, so a same-user process cannot read the guard's memory; it can read a process's
//! arguments and environment (`KERN_PROCARGS2`, `ps eww`) unless its own Seatbelt profile
//! denies `process-info`, so there too the peer check, not the secret alone, is the boundary.
//!
//! Windows: the guard's own process, and every child it starts, under a protected DACL that
//! lets no other process of the user read its memory, take its handles or change its DACL
//! (route 2, decision record 0060).
//!
//! A command in Codex's unelevated sandbox runs as the lawyer's own SID. The spike (row a1)
//! showed it can open an ordinary process of the user with `PROCESS_VM_READ`, so a secret in
//! the guard's memory (the relay's, `devtools.rs`) or a handle it holds (the browser's pipe)
//! would not be safe there. Row a1h: a process that set
//! `D:P(A;;0x1FFFFF;;;SY)(A;;0x101000;;;<user>)(A;;0x101000;;;OW)` on itself is denied
//! `PROCESS_VM_READ`, `PROCESS_DUP_HANDLE` and `WRITE_DAC` from the sandbox and from the user's
//! own shell; only `PROCESS_QUERY_LIMITED_INFORMATION` and `SYNCHRONIZE` are granted, which is
//! all the host's and the guard's own probes ask for (`record/windows.rs`, `parent/windows.rs`,
//! `clauseo-process`). The `P` blocks inherited entries; the `OW` (OWNER RIGHTS) entry takes away
//! the owner's implicit `READ_CONTROL | WRITE_DAC`.
//!
//! [`Hardened::apply`] sets that DACL on the guard first thing, before any secret exists
//! (`browser_guard.rs`), and keeps the descriptors for `CreateProcessW` (`job/spawn.rs`): its
//! `lpProcessAttributes` gets [`process_sddl`] and its `lpThreadAttributes` [`thread_sddl`], the
//! same entries with the thread rights that mean the same (`THREAD_QUERY_LIMITED_INFORMATION |
//! SYNCHRONIZE`; on a thread `0x1000` would be ``THREAD_RESUME``), so each child is hardened from
//! its first instruction. The guard keeps full access to what it creates: `CreateProcessW` hands
//! the creator all-access handles whatever the DACL says.
//!
//! Any failure refuses the browser: the guard exits before it starts anything.

/// Linux: no core dumps, and `/proc/<guard>/` and `ptrace` closed to the user's other processes.
#[cfg(target_os = "linux")]
pub(crate) fn not_dumpable() -> std::io::Result<()> {
    nix::sys::prctl::set_dumpable(false).map_err(std::io::Error::from)
}

/// `PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE`.
#[cfg(any(windows, test))]
const PROCESS_RIGHTS: u32 = 0x0010_1000;
/// `THREAD_QUERY_LIMITED_INFORMATION | SYNCHRONIZE`.
#[cfg(any(windows, test))]
const THREAD_RIGHTS: u32 = 0x0010_0800;

/// The process DACL for the user whose SID string is `user`.
#[cfg(any(windows, test))]
pub(crate) fn process_sddl(user: &str) -> String {
    sddl(user, PROCESS_RIGHTS)
}

/// The initial thread's DACL for the user whose SID string is `user`.
#[cfg(any(windows, test))]
pub(crate) fn thread_sddl(user: &str) -> String {
    sddl(user, THREAD_RIGHTS)
}

/// SYSTEM all access (`0x1FFFFF` is both `PROCESS_ALL_ACCESS` and `THREAD_ALL_ACCESS`); the user
/// and OWNER RIGHTS `rights`; protected.
#[cfg(any(windows, test))]
fn sddl(user: &str, rights: u32) -> String {
    format!("D:P(A;;0x1FFFFF;;;SY)(A;;0x{rights:X};;;{user})(A;;0x{rights:X};;;OW)")
}

#[cfg(windows)]
pub(crate) use windows_impl::Hardened;

#[cfg(windows)]
#[expect(
    unsafe_code,
    reason = "FFI to the Win32 security APIs, to set the guard's DACL and build its children's"
)]
mod windows_impl {
    use std::io;
    use std::os::windows::io::{FromRawHandle, OwnedHandle};

    use windows::Win32::Foundation::{HANDLE, HLOCAL, LocalFree};
    use windows::Win32::Security::Authorization::{
        ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
        SDDL_REVISION_1, SE_KERNEL_OBJECT, SetSecurityInfo,
    };
    use windows::Win32::Security::{
        ACL, DACL_SECURITY_INFORMATION, GetSecurityDescriptorDacl, GetTokenInformation,
        PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES,
        TOKEN_QUERY, TOKEN_USER, TokenUser,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    use windows::core::{BOOL, HSTRING, PWSTR};

    /// The two descriptors, built once and kept for the guard's life.
    #[derive(Debug)]
    pub(crate) struct Hardened {
        process: Descriptor,
        thread: Descriptor,
    }

    impl Hardened {
        /// Sets the protected DACL on the guard's own process and returns the descriptors for
        /// its children.
        pub(crate) fn apply() -> io::Result<Hardened> {
            let user = user_sid()?;
            let hardened = Hardened {
                process: Descriptor::from_sddl(&super::process_sddl(&user))?,
                thread: Descriptor::from_sddl(&super::thread_sddl(&user))?,
            };
            let dacl = hardened.process.dacl()?;
            // SAFETY: the pseudo handle of the current process needs no closing; `dacl` points
            // into the descriptor `hardened` owns, live for the call.
            let set = unsafe {
                SetSecurityInfo(
                    GetCurrentProcess(),
                    SE_KERNEL_OBJECT,
                    DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                    None,
                    None,
                    Some(dacl),
                    None,
                )
            };
            set.ok().map_err(io::Error::from)?;
            Ok(hardened)
        }

        /// `CreateProcessW`'s `lpProcessAttributes`; live while `self` is.
        pub(crate) fn process(&self) -> SECURITY_ATTRIBUTES {
            self.process.attributes()
        }

        /// `CreateProcessW`'s `lpThreadAttributes`; live while `self` is.
        pub(crate) fn thread(&self) -> SECURITY_ATTRIBUTES {
            self.thread.attributes()
        }
    }

    /// A self-relative security descriptor `ConvertStringSecurityDescriptorToSecurityDescriptorW`
    /// allocated, freed once on drop.
    #[derive(Debug)]
    struct Descriptor(PSECURITY_DESCRIPTOR);

    // SAFETY: the descriptor is written once, by its conversion, and only read afterwards (by
    // `SetSecurityInfo` and `CreateProcessW`); nothing else holds the pointer.
    unsafe impl Send for Descriptor {}
    // SAFETY: as for `Send`: shared reads of memory nothing writes.
    unsafe impl Sync for Descriptor {}

    impl Descriptor {
        fn from_sddl(sddl: &str) -> io::Result<Descriptor> {
            let mut descriptor = PSECURITY_DESCRIPTOR::default();
            // SAFETY: the string is a NUL-terminated HSTRING live for the call; `descriptor` is
            // a live pointer the call writes, freed by this value's drop.
            unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    &HSTRING::from(sddl),
                    SDDL_REVISION_1,
                    &raw mut descriptor,
                    None,
                )
            }
            .map_err(io::Error::from)?;
            Ok(Descriptor(descriptor))
        }

        /// The descriptor's DACL, which must be present.
        fn dacl(&self) -> io::Result<*const ACL> {
            let (mut present, mut defaulted) = (BOOL::default(), BOOL::default());
            let mut dacl: *mut ACL = std::ptr::null_mut();
            // SAFETY: `self.0` is a valid descriptor; the three outputs are live locals.
            unsafe {
                GetSecurityDescriptorDacl(
                    self.0,
                    &raw mut present,
                    &raw mut dacl,
                    &raw mut defaulted,
                )
            }
            .map_err(io::Error::from)?;
            if !present.as_bool() || dacl.is_null() {
                return Err(io::Error::other("the hardened descriptor has no DACL"));
            }
            Ok(dacl.cast_const())
        }

        fn attributes(&self) -> SECURITY_ATTRIBUTES {
            SECURITY_ATTRIBUTES {
                nLength: u32::try_from(size_of::<SECURITY_ATTRIBUTES>()).unwrap_or(0),
                lpSecurityDescriptor: self.0.0,
                bInheritHandle: false.into(),
            }
        }
    }

    impl Drop for Descriptor {
        fn drop(&mut self) {
            // SAFETY: `self.0` was allocated by LocalAlloc in its conversion and is freed here,
            // once.
            unsafe { LocalFree(Some(HLOCAL(self.0.0))) };
        }
    }

    /// The guard's user SID as a string (`S-1-5-21-...`).
    fn user_sid() -> io::Result<String> {
        let mut raw = HANDLE::default();
        // SAFETY: the pseudo handle needs no closing; `raw` is a live HANDLE the call writes.
        unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &raw mut raw) }
            .map_err(io::Error::from)?;
        // SAFETY: `raw` is the open token handle just returned, owned and closed once here.
        let token = unsafe { OwnedHandle::from_raw_handle(raw.0) };
        let token = HANDLE(std::os::windows::io::AsRawHandle::as_raw_handle(&token));
        let mut size = 0_u32;
        // SAFETY: with no buffer the call only writes the size it needs into `size`; its
        // ERROR_INSUFFICIENT_BUFFER is expected.
        drop(unsafe { GetTokenInformation(token, TokenUser, None, 0, &raw mut size) });
        let words = usize::try_from(size).map_err(io::Error::other)?.div_ceil(8);
        let mut buffer = vec![0_u64; words.max(1)];
        let length = u32::try_from(buffer.len() * 8).map_err(io::Error::other)?;
        // SAFETY: `buffer` is `length` writable bytes, aligned for TOKEN_USER, live for the call.
        unsafe {
            GetTokenInformation(
                token,
                TokenUser,
                Some(buffer.as_mut_ptr().cast()),
                length,
                &raw mut size,
            )
        }
        .map_err(io::Error::from)?;
        // SAFETY: the call filled `buffer` with a TOKEN_USER, whose SID points inside it.
        let sid = unsafe { (*buffer.as_ptr().cast::<TOKEN_USER>()).User.Sid };
        let mut text = PWSTR::null();
        // SAFETY: `sid` is valid while `buffer` lives; `text` is a live pointer the call sets to
        // a LocalAlloc'd string, freed below.
        unsafe { ConvertSidToStringSidW(sid, &raw mut text) }.map_err(io::Error::from)?;
        // SAFETY: `text` is the NUL-terminated string the call returned.
        let string = unsafe { text.to_string() }.map_err(io::Error::other);
        // SAFETY: `text` was allocated by LocalAlloc and is freed here, once.
        unsafe { LocalFree(Some(HLOCAL(text.0.cast()))) };
        string
    }
}

#[cfg(test)]
mod tests {
    use super::{process_sddl, thread_sddl};

    /// Route 2's row a1h, as the spike set it, character for character.
    #[test]
    fn the_process_dacl_is_the_spikes() {
        let user = "S-1-5-21-1004336348-1177238915-682003330-1001";
        assert_eq!(
            process_sddl(user),
            "D:P(A;;0x1FFFFF;;;SY)(A;;0x101000;;;S-1-5-21-1004336348-1177238915-682003330-1001)\
             (A;;0x101000;;;OW)"
        );
        assert_eq!(
            thread_sddl(user),
            "D:P(A;;0x1FFFFF;;;SY)(A;;0x100800;;;S-1-5-21-1004336348-1177238915-682003330-1001)\
             (A;;0x100800;;;OW)"
        );
    }
}
