//! A volume's mount flags read with `getattrlistat(2)`, which rustix doesn't have: the flags
//! of the volume mounted on a folder, read from the folder above it, without opening or
//! entering it.

#![expect(
    unsafe_code,
    reason = "getattrlistat(2): a mounted folder's volume flags without entering it"
)]

use std::ffi::{CString, OsStr, c_char, c_int, c_ulong, c_void};
use std::io;
use std::os::fd::{AsRawFd as _, OwnedFd};
use std::os::unix::ffi::OsStrExt as _;

/// `ATTR_BIT_MAP_COUNT` (`<sys/attr.h>`).
const ATTR_BIT_MAP_COUNT: u16 = 5;

/// `ATTR_VOL_INFO`: the volume attributes asked for are wanted.
const ATTR_VOL_INFO: u32 = 0x8000_0000;

/// `ATTR_VOL_MOUNTFLAGS`: the volume's `statfs` flags.
const ATTR_VOL_MOUNTFLAGS: u32 = 0x0000_4000;

/// `FSOPT_NOFOLLOW`: the last name is never followed.
const FSOPT_NOFOLLOW: c_ulong = 0x1;

/// `struct attrlist`.
#[repr(C)]
struct AttrList {
    bitmapcount: u16,
    reserved: u16,
    commonattr: u32,
    volattr: u32,
    dirattr: u32,
    fileattr: u32,
    forkattr: u32,
}

/// The reply: its length, then the one attribute asked for.
#[repr(C)]
struct Reply {
    length: u32,
    mount_flags: u32,
}

unsafe extern "C" {
    fn getattrlistat(
        fd: c_int,
        path: *const c_char,
        attr_list: *mut c_void,
        attr_buf: *mut c_void,
        attr_buf_size: usize,
        options: c_ulong,
    ) -> c_int;
}

/// The mount flags of the volume holding entry `name` of folder `dir`: for a folder another
/// volume is mounted on, that volume's. A lookup for attributes (`OP_GETATTR`) doesn't fire an
/// automount trigger (`os.rs`), and nothing is opened.
pub(super) fn mount_flags(dir: &OwnedFd, name: &OsStr) -> io::Result<u32> {
    let name = CString::new(name.as_bytes())?;
    let mut list = AttrList {
        bitmapcount: ATTR_BIT_MAP_COUNT,
        reserved: 0,
        commonattr: 0,
        volattr: ATTR_VOL_INFO | ATTR_VOL_MOUNTFLAGS,
        dirattr: 0,
        fileattr: 0,
        forkattr: 0,
    };
    let mut reply = Reply {
        length: 0,
        mount_flags: 0,
    };
    // SAFETY: `name` is NUL-terminated and outlives the call; `list` and `reply` are live
    // `repr(C)` values laid out as `<sys/attr.h>` has them, and the size given is `reply`'s.
    let done = unsafe {
        getattrlistat(
            dir.as_raw_fd(),
            name.as_ptr(),
            (&raw mut list).cast(),
            (&raw mut reply).cast(),
            size_of::<Reply>(),
            FSOPT_NOFOLLOW,
        )
    };
    if done != 0 {
        return Err(io::Error::last_os_error());
    }
    if usize::try_from(reply.length).unwrap_or_default() < size_of::<Reply>() {
        return Err(io::Error::other("getattrlistat gave no mount flags"));
    }
    Ok(reply.mount_flags)
}
