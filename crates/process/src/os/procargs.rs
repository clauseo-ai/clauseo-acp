//! The environment block of macOS's `KERN_PROCARGS2` answer, parsed: the process's argument
//! count (a native `int`), its executable's path, NUL padding, its arguments, then its
//! environment, each string ended by a NUL, the environment ended by an empty string (what
//! follows, the kernel's own `apple` strings, is not environment). Compiled on macOS, and for
//! the tests everywhere.

/// The environment entries of a `KERN_PROCARGS2` buffer, in order; `None` when the buffer ends
/// before its arguments do (a malformed or truncated answer).
pub(crate) fn environment(buffer: &[u8]) -> Option<Vec<&[u8]>> {
    let count = buffer.get(..size_of::<i32>())?;
    let argc = usize::try_from(i32::from_ne_bytes(count.try_into().ok()?)).ok()?;
    let rest = buffer.get(size_of::<i32>()..)?;
    // The executable's path, then the NULs that pad it.
    let path_end = rest.iter().position(|&byte| byte == 0)?;
    let rest = rest.get(path_end..)?;
    let first = rest.iter().position(|&byte| byte != 0)?;
    let mut strings = rest.get(first..)?.split(|&byte| byte == 0);
    for _ in 0..argc {
        strings.next()?;
    }
    Some(strings.take_while(|string| !string.is_empty()).collect())
}
