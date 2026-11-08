//! Windows' connectivity hints: `NotifyNetworkConnectivityHintChange` (iphlpapi) calls back on a
//! system thread with the aggregate connectivity level, and `CancelMibChangeNotify2` ends the
//! registration. The callback records the level and, on a change to internet access
//! ([`is_hint`]), only sends on the job's channel ([`Hint::send`]).
//!
//! The function is Windows 10 2004 (build 19041) and later, so it is looked up at run time: a
//! static import would stop the whole app from loading on an older Windows. Without it the
//! watch never subscribes, and the job retries on its timer alone.
#![expect(
    unsafe_code,
    reason = "FFI to iphlpapi's NotifyNetworkConnectivityHintChange and CancelMibChangeNotify2"
)]

use std::ffi::c_void;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicI32, Ordering};

use windows::Win32::Foundation::{HANDLE, NO_ERROR, WIN32_ERROR};
use windows::Win32::NetworkManagement::IpHelper::CancelMibChangeNotify2;
use windows::Win32::Networking::WinSock::NL_NETWORK_CONNECTIVITY_HINT;
use windows::Win32::System::LibraryLoader::{
    GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
};
use windows::core::{s, w};

use super::{ConnectivityWatch, Hint, Subscription, is_hint};

/// `NL_NETWORK_CONNECTIVITY_HINT` (netioapi.h): `ConnectivityLevel` and `ConnectivityCost` (C
/// enums, `int`), then `ApproachingDataLimit`, `OverDataLimit` and `Roaming` (`BOOLEAN`,
/// `unsigned char`), with C layout: 12 bytes, aligned to 4. Declared here with `u8`s, so no Rust
/// `bool` is ever read from the OS's bytes; the assertion below keeps it the SDK's size and
/// alignment.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct RawHint {
    level: i32,
    _cost: i32,
    _approaching_data_limit: u8,
    _over_data_limit: u8,
    _roaming: u8,
}

const _: () = assert!(
    size_of::<RawHint>() == size_of::<NL_NETWORK_CONNECTIVITY_HINT>()
        && align_of::<RawHint>() == align_of::<NL_NETWORK_CONNECTIVITY_HINT>()
);

/// `PNETWORK_CONNECTIVITY_HINT_CHANGE_CALLBACK`, taking [`RawHint`] by value.
type Callback = unsafe extern "system" fn(*const c_void, RawHint);

/// `NotifyNetworkConnectivityHintChange`'s signature (netioapi.h), with `InitialNotification`
/// as the `BOOLEAN` (`u8`) it is.
type NotifyFn =
    unsafe extern "system" fn(Option<Callback>, *const c_void, u8, *mut HANDLE) -> WIN32_ERROR;

/// No level recorded yet: the initial notice has not come.
const NO_LEVEL: i32 = i32::MIN;

/// What the callback reads: the job's channel and the last level seen.
struct Context {
    hint: Hint,
    last: AtomicI32,
}

/// The OS's connectivity hints.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct HintWatch;

impl ConnectivityWatch for HintWatch {
    fn subscribe(&self, hint: Hint) -> Option<Subscription> {
        let notify = notify_fn()?;
        let context = Box::into_raw(Box::new(Context {
            hint,
            last: AtomicI32::new(NO_LEVEL),
        }));
        let mut handle = HANDLE::default();
        // SAFETY: `notify` is the export of that name, of `NotifyFn`'s signature, and `on_hint`
        // is of the callback's. `context` points to a live `Context` that stays alive until the
        // registration is cancelled (`Registered::drop`); `handle` is a valid, writable HANDLE.
        // The initial notice (1) only records the level the network is at now.
        let result = unsafe {
            notify(
                Some(on_hint),
                context.cast_const().cast(),
                1,
                &raw mut handle,
            )
        };
        if result != NO_ERROR {
            // SAFETY: the registration failed, so the OS holds no copy of `context`, which came
            // from `Box::into_raw` above and is freed once, here.
            drop(unsafe { Box::from_raw(context) });
            tracing::debug!(code = result.0, "connectivity hints: no registration");
            return None;
        }
        Some(Subscription::new(Registered { handle, context }))
    }
}

/// A registration, cancelled when it drops.
struct Registered {
    handle: HANDLE,
    context: *mut Context,
}

// SAFETY: the registration handle may be cancelled from any thread, and `context` (a `Hint` and
// an atomic, both `Send` and `Sync`) is only freed by `drop`, after the cancel.
unsafe impl Send for Registered {}

impl Drop for Registered {
    fn drop(&mut self) {
        // SAFETY: `handle` came from a successful registration and is cancelled once, here.
        let result = unsafe { CancelMibChangeNotify2(self.handle) };
        if result == NO_ERROR {
            // SAFETY: that no callback for this handle runs once CancelMibChangeNotify2 has
            // returned is inferred, not documented for this function: its page lists only the
            // older `Notify*` functions, and says that calling it from the callback for the same
            // handle deadlocks, which means it waits for a callback in flight. The deadlock's
            // conditions are not met here: the cancel runs only on the job's thread (the
            // subscription drops there, never in the callback); the callback only makes a
            // non-blocking send on an unbounded channel and an atomic swap; the two share no
            // lock; and the job's receiver is dropped before the subscription (`Listening`), so
            // a late send only fails. Then nothing reads `context` any more. It came from
            // `Box::into_raw` in `subscribe` and is freed once, here.
            drop(unsafe { Box::from_raw(self.context) });
        } else {
            // The registration may still call back: `context` is leaked, so it stays valid.
            tracing::debug!(code = result.0, "connectivity hints: cancel failed");
        }
    }
}

/// The callback, on a system thread: records the level, and sends a hint on a change to internet
/// access. It never blocks and takes no lock.
unsafe extern "system" fn on_hint(context: *const c_void, hint: RawHint) {
    if context.is_null() {
        return;
    }
    // SAFETY: `context` is the `Context` registered with this callback, alive until the
    // registration is cancelled (`Registered::drop`). `Context` is `Sync`, so calls on several
    // threads at once may share it.
    let context = unsafe { &*context.cast::<Context>() };
    let last = context.last.swap(hint.level, Ordering::AcqRel);
    if is_hint((last != NO_LEVEL).then_some(last), hint.level) {
        context.hint.send();
    }
}

/// `NotifyNetworkConnectivityHintChange`, looked up once in iphlpapi; `None` before build 19041.
fn notify_fn() -> Option<NotifyFn> {
    static FOUND: OnceLock<Option<NotifyFn>> = OnceLock::new();
    *FOUND.get_or_init(|| {
        // SAFETY: the name is a static NUL-terminated wide string; the search is limited to
        // System32. The module is never freed, so the address below stays valid.
        let module =
            unsafe { LoadLibraryExW(w!("iphlpapi.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32) }
                .ok()?;
        // SAFETY: `module` is loaded, and the name is a static NUL-terminated string.
        let found = unsafe { GetProcAddress(module, s!("NotifyNetworkConnectivityHintChange")) }?;
        // SAFETY: the export of that name has `NotifyFn`'s signature (netioapi.h, with
        // `RawHint` laid out as the SDK's struct); both are `extern "system"` function pointers
        // of the same size.
        Some(unsafe {
            std::mem::transmute::<unsafe extern "system" fn() -> isize, NotifyFn>(found)
        })
    })
}
