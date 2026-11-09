//! The git watch's Win32 half: `ReadDirectoryChangesExW` on a watched folder, overlapped, so a
//! wait can also watch the stop event (as the runner pipes' waits do, `pipe/os.rs`). It asks
//! for `ReadDirectoryNotifyExtendedInformation`, whose records carry each item's attributes,
//! into a 1 MiB buffer; a volume that refuses either (a network share takes at most 64 KiB, and
//! may not give the extended records) gets 64 KiB, then `ReadDirectoryChangesW`'s own records.
#![expect(
    unsafe_code,
    reason = "FFI to ReadDirectoryChangesW and the overlapped wait"
)]

use std::io;
use std::path::Path;
use std::time::{Duration, SystemTime};

use windows::Win32::Foundation::{
    ERROR_INVALID_FUNCTION, ERROR_INVALID_PARAMETER, ERROR_NOT_SUPPORTED, ERROR_NOTIFY_ENUM_DIR,
    HANDLE, WAIT_OBJECT_0,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OVERLAPPED, FILE_LIST_DIRECTORY,
    FILE_NOTIFY_CHANGE_ATTRIBUTES, FILE_NOTIFY_CHANGE_DIR_NAME, FILE_NOTIFY_CHANGE_FILE_NAME,
    FILE_NOTIFY_CHANGE_LAST_WRITE, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    OPEN_EXISTING, ReadDirectoryChangesExW, ReadDirectoryChangesW,
    ReadDirectoryNotifyExtendedInformation,
};
use windows::Win32::System::Threading::{
    CreateEventW, INFINITE, SetEvent, WaitForMultipleObjects, WaitForSingleObject,
};
use windows::core::HSTRING;

use super::records::{Layout, filetime};
use super::{Next, Source, changes};
use crate::windows::acl::object::{Handle, os_error};
use crate::windows::pipe::os::Overlapped;

/// The buffer the changes are read into on a local volume: 1 MiB, so a flood of changes between
/// two reads overflows (and rescans) far less often.
const LOCAL_WORDS: usize = 128 * 1024;
/// The most `ReadDirectoryChangesW` takes for a folder on a network share: 64 KiB.
const SHARE_WORDS: usize = 8 * 1024;
/// What each try at arming asks for, in turn, while the volume refuses the one before.
const TRIES: [(Layout, usize); 3] = [
    (Layout::Extended, LOCAL_WORDS),
    (Layout::Extended, SHARE_WORDS),
    (Layout::Plain, SHARE_WORDS),
];

/// The watch's stop: a manual-reset event the outer sets at the session's end.
pub(crate) struct Stop(Handle);

// SAFETY: an event handle is a kernel handle any thread may use; it is closed once, when the
// last `Arc` of it drops.
unsafe impl Send for Stop {}
// SAFETY: as above; `SetEvent` and the waits on it may run on two threads at once.
unsafe impl Sync for Stop {}

impl Stop {
    pub(crate) fn new() -> io::Result<Stop> {
        // SAFETY: no attributes and no name; the event is owned by `Handle`.
        let event = unsafe { CreateEventW(None, true, false, None) }.map_err(|e| os_error(&e))?;
        Ok(Stop(Handle(event)))
    }

    /// Stops the watch: its wait ends, and so does any pause.
    pub(crate) fn set(&self) {
        // SAFETY: the event is live.
        if let Err(error) = unsafe { SetEvent(self.0.0) } {
            tracing::debug!(code = error.code().0, "SetEvent failed");
        }
    }

    /// Waits up to `wait`; whether the watch is stopped.
    pub(crate) fn wait(&self, wait: Duration) -> bool {
        let millis = u32::try_from(wait.as_millis()).unwrap_or(u32::MAX);
        // SAFETY: the event is live for the wait.
        unsafe { WaitForSingleObject(self.0.0, millis) == WAIT_OBJECT_0 }
    }

    fn raw(&self) -> HANDLE {
        self.0.0
    }
}

/// The matter's folder, opened to list (`FILE_LIST_DIRECTORY`, `FILE_FLAG_BACKUP_SEMANTICS`)
/// for overlapped reads of its changes, and the read in flight.
pub(crate) struct DirWatch<'a> {
    folder: Handle,
    buffer: Box<[u64]>,
    layout: Layout,
    pending: Option<Overlapped>,
    stop: &'a Stop,
}

impl<'a> DirWatch<'a> {
    /// Opens `root` and starts the first read, so a watch that can't be armed fails here.
    pub(crate) fn arm(root: &Path, stop: &'a Stop) -> io::Result<DirWatch<'a>> {
        let share = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;
        // SAFETY: the name is a live HSTRING; no security attributes or template are passed,
        // and the handle is owned by `Handle` from here.
        let folder = unsafe {
            CreateFileW(
                &HSTRING::from(root.as_os_str()),
                FILE_LIST_DIRECTORY.0,
                share,
                None,
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED,
                None,
            )
        }
        .map_err(|e| os_error(&e))?;
        let mut watch = DirWatch {
            folder: Handle(folder),
            buffer: Box::default(),
            layout: Layout::Extended,
            pending: None,
            stop,
        };
        let mut last = io::Error::other("no read tried");
        for (layout, words) in TRIES {
            watch.layout = layout;
            watch.buffer = vec![0; words].into_boxed_slice();
            match watch.read() {
                Ok(()) => return Ok(watch),
                Err(error) if refused(&error) => last = error,
                Err(error) => return Err(error),
            }
        }
        Err(last)
    }

    /// Starts the next read, of names made, renamed or removed, attributes and writes anywhere
    /// below the folder: a junction set on a folder changes its attributes.
    fn read(&mut self) -> io::Result<()> {
        let mut io = Overlapped::new()?;
        let length = u32::try_from(size_of_val(&*self.buffer)).map_err(io::Error::other)?;
        let filter = FILE_NOTIFY_CHANGE_DIR_NAME
            | FILE_NOTIFY_CHANGE_FILE_NAME
            | FILE_NOTIFY_CHANGE_ATTRIBUTES
            | FILE_NOTIFY_CHANGE_LAST_WRITE;
        let buffer = self.buffer.as_mut_ptr().cast();
        // SAFETY: the folder is open for overlapped I/O; the buffer is boxed, `u64`-aligned and
        // `length` bytes long, and it and `io` stay in `self` until the read is waited for or
        // cancelled (`next`, `Drop`).
        let started = unsafe {
            match self.layout {
                Layout::Extended => ReadDirectoryChangesExW(
                    self.folder.0,
                    buffer,
                    length,
                    true,
                    filter,
                    None,
                    Some(io.as_mut_ptr()),
                    None,
                    ReadDirectoryNotifyExtendedInformation,
                ),
                Layout::Plain => ReadDirectoryChangesW(
                    self.folder.0,
                    buffer,
                    length,
                    true,
                    filter,
                    None,
                    Some(io.as_mut_ptr()),
                    None,
                ),
            }
        };
        started.map_err(|e| os_error(&e))?;
        self.pending = Some(io);
        Ok(())
    }
}

impl Source for DirWatch<'_> {
    fn next(&mut self) -> Next {
        if self.pending.is_none()
            && let Err(error) = self.read()
        {
            return Next::Broken(error);
        }
        let Some(io) = self.pending.as_ref() else {
            return Next::Broken(io::Error::other("no read in flight"));
        };
        // SAFETY: both events are live for the wait.
        let woke =
            unsafe { WaitForMultipleObjects(&[io.event.0, self.stop.raw()], false, INFINITE) };
        if woke != WAIT_OBJECT_0 {
            return Next::Stopped;
        }
        let Some(io) = self.pending.take() else {
            return Next::Broken(io::Error::other("no read in flight"));
        };
        match io.result(&self.folder) {
            Ok(0) => Next::Overflow,
            Ok(read) => {
                // Once per buffer, as it is received: each folder added is told moved in or made
                // by its creation time against this.
                let now = filetime(wall_clock());
                let bytes = bytes_of(&self.buffer);
                bytes
                    .get(..read)
                    .and_then(|read| changes(read, self.layout, now))
                    .map_or(Next::Overflow, Next::Changed)
            }
            Err(error)
                if error
                    .raw_os_error()
                    .is_some_and(|code| i64::from(code) == i64::from(ERROR_NOTIFY_ENUM_DIR.0)) =>
            {
                Next::Overflow
            }
            Err(error) => Next::Broken(error),
        }
    }
}

impl Drop for DirWatch<'_> {
    fn drop(&mut self) {
        if let Some(io) = self.pending.take() {
            io.cancel(&self.folder);
        }
    }
}

/// Whether `error` is a volume refusing what the read asked for, so a smaller or plainer one may
/// do.
fn refused(error: &io::Error) -> bool {
    let code = error
        .raw_os_error()
        .and_then(|code| u32::try_from(code).ok());
    [
        ERROR_INVALID_PARAMETER,
        ERROR_INVALID_FUNCTION,
        ERROR_NOT_SUPPORTED,
    ]
    .iter()
    .any(|refused| code == Some(refused.0))
}

/// The wall clock, read once per buffer as it is received: a folder reported added that was
/// created more than `records::MOVED_IN_AGE` before was moved in, not made.
#[expect(
    clippy::disallowed_methods,
    reason = "a folder moved in is told from one made by its creation time against the wall \
              clock; the tests pass the time to `records::changes`"
)]
fn wall_clock() -> SystemTime {
    SystemTime::now()
}

/// The buffer's bytes.
fn bytes_of(words: &[u64]) -> &[u8] {
    // SAFETY: any `u64` is eight valid bytes; the slice covers exactly the array and borrows it.
    unsafe { std::slice::from_raw_parts(words.as_ptr().cast::<u8>(), size_of_val(words)) }
}
