//! What the refresh reads from the install folder (step 5): the two files, and why one was
//! refused.

/// Why the user folder's exe or `.sig` was refused. Names no path.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SourceError {
    /// Not there.
    #[error("the file isn't there")]
    Missing,
    /// A reparse point (a link, a junction, a cloud placeholder, ...) at the file or on the way
    /// to it; tag 0 when the open met it on the way.
    #[error("a reparse point (tag {0:#x}) is in the way")]
    ReparsePoint(u32),
    /// A folder, a device, or anything but a regular file.
    #[error("the file isn't a regular file")]
    NotRegular,
    /// More than one hard link.
    #[error("the file has {0} hard links")]
    Linked(u32),
    /// Over its cap.
    #[error("the file is over {0} bytes")]
    TooLarge(u64),
    /// A Windows call failed.
    #[error("{call} failed ({code})")]
    Os {
        /// The call.
        call: &'static str,
        /// Its Win32 error code.
        code: u32,
    },
}

/// Which file of the user folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceFile {
    /// The folder chain itself, before either file.
    Folder,
    /// `clauseo-sandbox-setup.exe`.
    Exe,
    /// `clauseo-sandbox-setup.exe.sig`.
    Sig,
}

/// The two files read from the install folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sources {
    /// The exe's bytes.
    pub exe: Vec<u8>,
    /// The `.sig`'s bytes.
    pub sig: Vec<u8>,
}
