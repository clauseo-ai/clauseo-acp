//! The git watch's `FSEvents` stream: a minimal binding to `CoreServices`' `FSEventStream*`,
//! run on a serial dispatch queue of its own, with file events
//! (`kFSEventStreamCreateFlagFileEvents`), no defer, the roots watched and a 0.1 s latency: the
//! matter, and the canary's folder when that is outside it (`run.rs`). Each
//! callback's batch goes down the session's channel as [`Msg::Events`], stamped as it is
//! queued ([`Msg::queued`]), so the runner sees how far behind it reads. The workspace has no
//! `FSEvents` crate; this is the one place the watch calls into `CoreServices`.

#![expect(
    unsafe_code,
    reason = "the FSEvents and CoreFoundation calls the git watch's stream needs"
)]

use std::ffi::{CStr, c_char, c_void};
use std::os::unix::ffi::OsStrExt as _;
use std::path::PathBuf;
use std::ptr;
use std::sync::Arc;
use std::sync::mpsc::Sender;

use super::Event;
use super::run::Msg;

/// The stream's latency, in seconds.
const LATENCY: f64 = 0.1;

/// `kFSEventStreamCreateFlagNoDefer | kFSEventStreamCreateFlagWatchRoot |
/// kFSEventStreamCreateFlagFileEvents`.
const CREATE_FLAGS: u32 = 0x02 | 0x04 | 0x10;

/// `kFSEventStreamEventIdSinceNow`.
const SINCE_NOW: u64 = u64::MAX;

/// `kCFStringEncodingUTF8`.
const UTF8: u32 = 0x0800_0100;

type Callback =
    extern "C" fn(*const c_void, *mut c_void, usize, *mut c_void, *const u32, *const u64);

/// `FSEventStreamContext`.
#[repr(C)]
struct Context {
    version: isize,
    info: *mut c_void,
    retain: Option<extern "C" fn(*const c_void) -> *const c_void>,
    release: Option<extern "C" fn(*const c_void)>,
    copy_description: Option<extern "C" fn(*const c_void) -> *const c_void>,
}

#[link(name = "CoreServices", kind = "framework")]
unsafe extern "C" {
    fn FSEventStreamCreate(
        allocator: *const c_void,
        callback: Callback,
        context: *const Context,
        paths: *const c_void,
        since: u64,
        latency: f64,
        flags: u32,
    ) -> *mut c_void;
    fn FSEventStreamSetDispatchQueue(stream: *mut c_void, queue: *mut c_void);
    fn FSEventStreamStart(stream: *mut c_void) -> u8;
    fn FSEventStreamStop(stream: *mut c_void);
    fn FSEventStreamInvalidate(stream: *mut c_void);
    fn FSEventStreamRelease(stream: *mut c_void);
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    static kCFTypeArrayCallBacks: c_void;
    fn CFStringCreateWithBytes(
        allocator: *const c_void,
        bytes: *const u8,
        len: isize,
        encoding: u32,
        external: u8,
    ) -> *const c_void;
    fn CFArrayCreate(
        allocator: *const c_void,
        values: *const *const c_void,
        count: isize,
        callbacks: *const c_void,
    ) -> *const c_void;
    fn CFRelease(object: *const c_void);
}

unsafe extern "C" {
    fn dispatch_queue_create(label: *const c_char, attr: *const c_void) -> *mut c_void;
    fn dispatch_release(object: *mut c_void);
}

/// The channel each callback sends to; the stream holds one count of its `Arc`.
type Info = Sender<Msg>;

/// A started stream. Dropping it stops, invalidates and releases it; the stream releases its
/// count of the channel when it is freed, so no callback outlives the channel.
#[derive(Debug)]
pub(crate) struct Stream {
    stream: *mut c_void,
    queue: *mut c_void,
}

// SAFETY: FSEventStream calls are documented safe from any thread once the stream is on a
// dispatch queue; the pointers are only used to stop and release it, once, on drop.
unsafe impl Send for Stream {}

impl Drop for Stream {
    fn drop(&mut self) {
        // SAFETY: `stream` was created, put on `queue` and started by `start`, and is released
        // here once; `queue` was created by `start` and the stream no longer uses it once
        // invalidated.
        unsafe {
            FSEventStreamStop(self.stream);
            FSEventStreamInvalidate(self.stream);
            FSEventStreamRelease(self.stream);
            dispatch_release(self.queue);
        }
    }
}

extern "C" fn retain(info: *const c_void) -> *const c_void {
    // SAFETY: `info` is the `Arc<Info>` pointer `start` made with `Arc::into_raw`, still alive.
    unsafe { Arc::increment_strong_count(info.cast::<Info>()) };
    info
}

extern "C" fn release(info: *const c_void) {
    // SAFETY: as in `retain`; each release matches one retain or `start`'s own count.
    unsafe { Arc::decrement_strong_count(info.cast::<Info>()) };
}

extern "C" fn callback(
    _stream: *const c_void,
    info: *mut c_void,
    count: usize,
    paths: *mut c_void,
    flags: *const u32,
    _ids: *const u64,
) {
    if info.is_null() || paths.is_null() || flags.is_null() {
        return;
    }
    let paths = paths.cast::<*const c_char>();
    let mut events = Vec::with_capacity(count);
    for i in 0..count {
        // SAFETY: without kFSEventStreamCreateFlagUseCFTypes, `paths` is an array of `count`
        // C strings and `flags` of `count` flags, valid for the callback.
        let (path, flags) = unsafe { (CStr::from_ptr(*paths.add(i)), *flags.add(i)) };
        let path = PathBuf::from(std::ffi::OsStr::from_bytes(path.to_bytes()));
        events.push(Event { path, flags });
    }
    // SAFETY: the stream holds a count of the `Arc<Info>` behind `info` while it can call back.
    let tx = unsafe { &*info.cast::<Info>().cast_const() };
    if tx.send(Msg::queued(events)).is_err() {
        // The session's thread is gone; its drop of the stream follows.
    }
}

/// Creates and starts one stream on `roots`, sending its batches down `tx`.
pub(crate) fn start(roots: &[PathBuf], tx: Sender<Msg>) -> Result<Stream, String> {
    let mut lens = Vec::with_capacity(roots.len());
    for root in roots {
        let len = root.as_os_str().as_bytes().len();
        lens.push(isize::try_from(len).map_err(|error| error.to_string())?);
    }
    let count = isize::try_from(roots.len()).map_err(|error| error.to_string())?;
    let info = Arc::into_raw(Arc::new(tx));
    let context = Context {
        version: 0,
        info: info.cast_mut().cast::<c_void>(),
        retain: Some(retain),
        release: Some(release),
        copy_description: None,
    };
    // SAFETY: plain CoreFoundation calls on valid pointers; every object made here is released
    // here except the stream and the queue, which `Stream` owns. `start`'s own count of `info`
    // is released at the end: the stream retained it if it was made.
    let made = unsafe {
        let mut values = Vec::with_capacity(roots.len());
        for (root, len) in roots.iter().zip(&lens) {
            let bytes = root.as_os_str().as_bytes();
            let string = CFStringCreateWithBytes(ptr::null(), bytes.as_ptr(), *len, UTF8, 0);
            if string.is_null() {
                for value in &values {
                    CFRelease(*value);
                }
                release(info.cast());
                return Err(format!("{} is not UTF-8", root.display()));
            }
            values.push(string);
        }
        let array = CFArrayCreate(
            ptr::null(),
            values.as_ptr(),
            count,
            (&raw const kCFTypeArrayCallBacks).cast(),
        );
        for value in &values {
            CFRelease(*value);
        }
        if array.is_null() {
            release(info.cast());
            return Err("CFArrayCreate failed".to_owned());
        }
        let stream = FSEventStreamCreate(
            ptr::null(),
            callback,
            &raw const context,
            array,
            SINCE_NOW,
            LATENCY,
            CREATE_FLAGS,
        );
        CFRelease(array);
        release(info.cast());
        if stream.is_null() {
            return Err("FSEventStreamCreate failed".to_owned());
        }
        let queue = dispatch_queue_create(c"chat.clauseo.git-watch".as_ptr(), ptr::null());
        if queue.is_null() {
            FSEventStreamRelease(stream);
            return Err("dispatch_queue_create failed".to_owned());
        }
        FSEventStreamSetDispatchQueue(stream, queue);
        if FSEventStreamStart(stream) == 0 {
            FSEventStreamInvalidate(stream);
            FSEventStreamRelease(stream);
            dispatch_release(queue);
            return Err("FSEventStreamStart failed".to_owned());
        }
        Stream { stream, queue }
    };
    Ok(made)
}
