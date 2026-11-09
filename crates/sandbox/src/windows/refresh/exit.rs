//! The refresh's exit codes ([`RefreshExit`]), and the compile-time check that ties
//! [`RefreshExit::ALL`] to them.

/// The refresh's exit codes. They start at 32, clear of [`SetupExit`]'s, so the two never share
/// a number whatever either appends; codes are appended from here on, never renumbered.
///
/// [`SetupExit`]: crate::windows::setup::SetupExit
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshExit {
    /// 0: the admin copy is current (refreshed, already current, or its record written).
    Done,
    /// 32: not LOCAL SYSTEM, or not run from the admin copy's path.
    NotAllowed,
    /// 33: the record can't be read, or lacks a usable `AdminCopyOwner`, `UserInstallDir` or
    /// `AdminCopyVersion`.
    NoRecord,
    /// 34: the user folder's exe or `.sig` was refused: missing, a reparse point on the way or
    /// at the file, not a regular file, more than one link, over its cap, or unreadable.
    SourceRefused,
    /// 35: the staged exe's signature was refused (`SignatureError`).
    SignatureRefused,
    /// 36: this build's updater key is the placeholder; nothing was read.
    PlaceholderKey,
    /// 37: the staging folder or the staged copy couldn't be made, read back or secured.
    StagingFailed,
    /// 38: the admin copy couldn't be replaced; the old copy is in place.
    ReplaceFailed,
    /// 39: the new copy is in place, but the record's version or hash couldn't be written; the
    /// launcher refuses the copy until the next run writes them.
    RecordNotWritten,
    /// 40: the signed version is older than the record's `AdminCopyVersion` (or the same, with
    /// other bytes), or a version or the `FILEVERSION` can't be read.
    Downgrade,
    /// 41: `UserInstallDir` isn't inside the owner's profile, or the owner has none; for a
    /// `machine` owner, it isn't a folder only administrators can change.
    OutsideProfile,
    /// 42: the staged exe is for another CPU type than this copy, and isn't the one allowed move
    /// (`machine.rs`: x64 to a newer ARM64 build on an ARM64 machine).
    WrongMachine,
    /// 43: the admin folder couldn't be held for the renames (someone holds it); nothing moved.
    FolderBusy,
    /// 44: the set-up lock is held (an `--adopt`, set-up or removal is running), or can't be
    /// taken; nothing was read or changed.
    LockHeld,
}

impl RefreshExit {
    /// Every variant, in code order. Appended with each new one: a compile-time check below fails
    /// the build unless this lists every code from 32 up, in order.
    pub const ALL: [RefreshExit; 14] = [
        RefreshExit::Done,
        RefreshExit::NotAllowed,
        RefreshExit::NoRecord,
        RefreshExit::SourceRefused,
        RefreshExit::SignatureRefused,
        RefreshExit::PlaceholderKey,
        RefreshExit::StagingFailed,
        RefreshExit::ReplaceFailed,
        RefreshExit::RecordNotWritten,
        RefreshExit::Downgrade,
        RefreshExit::OutsideProfile,
        RefreshExit::WrongMachine,
        RefreshExit::FolderBusy,
        RefreshExit::LockHeld,
    ];

    /// The process exit code.
    pub const fn as_code(self) -> u8 {
        match self {
            RefreshExit::Done => 0,
            RefreshExit::NotAllowed => 32,
            RefreshExit::NoRecord => 33,
            RefreshExit::SourceRefused => 34,
            RefreshExit::SignatureRefused => 35,
            RefreshExit::PlaceholderKey => 36,
            RefreshExit::StagingFailed => 37,
            RefreshExit::ReplaceFailed => 38,
            RefreshExit::RecordNotWritten => 39,
            RefreshExit::Downgrade => 40,
            RefreshExit::OutsideProfile => 41,
            RefreshExit::WrongMachine => 42,
            RefreshExit::FolderBusy => 43,
            RefreshExit::LockHeld => 44,
        }
    }
}

// `RefreshExit::ALL` tied to the enum at compile time: `as_code`'s match is exhaustive, so a new
// variant doesn't build until it has a code (the next one; append it to `ALL` too), and the build
// fails unless `ALL` holds 0, then 32 up, in order with no gap or repeat.
const _: () = {
    let (mut rest, mut code): (&[RefreshExit], u8) = (&RefreshExit::ALL, 0);
    while let [exit, tail @ ..] = rest {
        assert!(exit.as_code() == code, "RefreshExit::ALL is out of order");
        code = if code == 0 { 32 } else { code + 1 };
        rest = tail;
    }
};
