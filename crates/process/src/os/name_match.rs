//! Which listed process is the one asked for by name, for macOS's `named` (`macos.rs`), as a
//! plain function of what libproc answered, so every row is tested on every OS.

/// `MAXCOMLEN`: how many bytes of the executable's name the kernel keeps as `p_comm`, which
/// `PROC_PIDT_SHORTBSDINFO`'s `pbsi_comm` carries (`Microsoft Word`, 14 bytes, fits).
pub(crate) const MAXCOMLEN: usize = 16;

/// What one listed process is, for a lookup by name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// Its executable's file name is the name: read its start.
    Named,
    /// It is not the process asked for, or it has exited.
    Other,
    /// It runs and may be the one, but which it is cannot be read: the whole answer is unknown.
    Unknown,
}

/// What `PROC_PIDT_SHORTBSDINFO` said of a process: its `pbsi_comm` bytes before any NUL, and
/// whether it has exited and waits to be reaped (`SZOMB`).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Short<'a> {
    pub(crate) comm: &'a [u8],
    pub(crate) exited: bool,
}

/// The kind of one listed process, looked up as `name`:
/// - its path read (`file`, the executable's file name): [`Kind::Named`] when it is `name`,
///   byte for byte, else [`Kind::Other`];
/// - no path (an app bundle replaced under a running process, a zombie, a race with its exit):
///   by `short`: exited is [`Kind::Other`]; a `comm` that is `name` cut to [`MAXCOMLEN`]
///   bytes is [`Kind::Unknown`] (a running Word whose path cannot be read); any other `comm` is
///   [`Kind::Other`];
/// - neither read: [`Kind::Other`] when `gone` says it has exited, else [`Kind::Unknown`].
///
/// `gone` is asked only in that last case.
pub(crate) fn kind(
    name: &str,
    file: Option<&[u8]>,
    short: Option<Short<'_>>,
    gone: impl FnOnce() -> bool,
) -> Kind {
    if let Some(file) = file {
        return if file == name.as_bytes() {
            Kind::Named
        } else {
            Kind::Other
        };
    }
    match short {
        Some(Short { exited: true, .. }) => Kind::Other,
        Some(Short { comm, .. }) => {
            let cut = name.as_bytes().get(..MAXCOMLEN).unwrap_or(name.as_bytes());
            if comm == cut {
                Kind::Unknown
            } else {
                Kind::Other
            }
        }
        None if gone() => Kind::Other,
        None => Kind::Unknown,
    }
}

#[cfg(test)]
mod tests;
