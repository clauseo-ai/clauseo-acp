//! A plain file read whole through its own handle (beside `object.rs`), for
//! [`Object::read_child`] and [`Object::read_child_secured`]: the latter reads the file's
//! security descriptor through the same handle too, so what it says is of the file whose bytes
//! were read.

use std::io;

use windows::Win32::Storage::FileSystem::{
    FILE_READ_ATTRIBUTES, FILE_READ_DATA, READ_CONTROL, ReadFile, SYNCHRONIZE,
};

use super::{Object, open_at, os_error};

/// The plain file `name` in `folder`, opened relative to its handle ([`open_at`], never through
/// a link) for reading, and read whole, with the parts of its security descriptor `info` asks
/// for read first (empty when `info` is 0); `None` when it is missing. A reparse point, a
/// folder, a file with another name, or one over `cap` bytes is refused.
pub(super) fn read_child(
    folder: &Object,
    name: &[u16],
    cap: usize,
    info: u32,
) -> io::Result<Option<(Vec<u8>, Vec<u8>)>> {
    let access = FILE_READ_DATA.0 | FILE_READ_ATTRIBUTES.0 | READ_CONTROL.0 | SYNCHRONIZE.0;
    let handle = match open_at(&folder.handle, name, access, true) {
        Ok(handle) => handle,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let file = Object::describe(handle, false, false)?;
    if file.folder || file.tag.is_some() || file.links > 1 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "not a plain file",
        ));
    }
    let security = if info == 0 {
        Vec::new()
    } else {
        file.security(info)?
    };
    let mut bytes = vec![0u8; cap.saturating_add(1)];
    let mut filled = 0;
    loop {
        let mut read = 0u32;
        let room = bytes.get_mut(filled..).unwrap_or_default();
        // SAFETY: the handle is open for synchronous reads with FILE_READ_DATA; `room` is a
        // live, writable slice and `read` a live out-pointer.
        unsafe { ReadFile(file.handle.0, Some(room), Some(&raw mut read), None) }
            .map_err(|e| os_error(&e))?;
        if read == 0 {
            break;
        }
        filled += usize::try_from(read).map_err(io::Error::other)?;
        if filled > cap {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "larger than its cap",
            ));
        }
    }
    bytes.truncate(filled);
    Ok(Some((bytes, security)))
}
