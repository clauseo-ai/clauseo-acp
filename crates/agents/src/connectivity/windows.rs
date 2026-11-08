//! Windows' connectivity hints: `NotifyNetworkConnectivityHintChange` (iphlpapi) calls back on a
//! system thread at each change of the aggregate connectivity level, and `CancelMibChangeNotify2`
//! ends the registration. The callback only sends on the job's channel ([`Hint::send`]).
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

use windows::Win32::Foundation::{HANDLE, NO_ERROR, WIN32_ERROR};
use windows::Win32::NetworkManagement::IpHelper::{
    CancelMibChangeNotify2, PNETWORK_CONNECTIVITY_HINT_CHANGE_CALLBACK,
};
use windows::Win32::Networking::WinSock::{
    NL_NETWORK_CONNECTIVITY_HINT, NetworkConnectivityLevelHintNone,
};
use windows::Win32::System::LibraryLoader::{
    GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
};
use windows::core::{s, w};

use super::{ConnectivityWatch, Hint, Subscription};

/// `NotifyNetworkConnectivityHintChange`'s signature (netioapi.h).
type NotifyFn = unsafe extern "system" fn(
    PNETWORK_CONNECTIVITY_HINT_CHANGE_CALLBACK,
    *const c_void,
    bool,
    *mut HANDLE,
) -> WIN32_ERROR;

/// The OS's connectivity hints.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct HintWatch;

impl ConnectivityWatch for HintWatch {
    fn subscribe(&self, hint: Hint) -> Option<Subscription> {
        let notify = notify_fn()?;
        let context = Box::into_raw(Box::new(hint));
        let mut handle = HANDLE::default();
        // SAFETY: `notify` is the export of that name, of that signature. `on_hint` matches the
        // callback type; `context` points to a live `Hint` that stays alive until the
        // registration is cancelled (`Registered::drop`); `handle` is a valid, writable HANDLE.
        // No initial notification: only a change after this call is a hint.
        let result = unsafe {
            notify(
                Some(on_hint),
                context.cast_const().cast(),
                false,
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
    context: *mut Hint,
}

// SAFETY: the registration handle may be cancelled from any thread, and `context` (a `Hint`,
// which is `Send` and `Sync`) is only freed by `drop`, after the OS has stopped calling back.
unsafe impl Send for Registered {}

impl Drop for Registered {
    fn drop(&mut self) {
        // SAFETY: `handle` came from a successful registration and is cancelled once, here,
        // never from the callback (which would deadlock).
        let result = unsafe { CancelMibChangeNotify2(self.handle) };
        if result == NO_ERROR {
            // SAFETY: once CancelMibChangeNotify2 has returned, no callback for this handle is
            // running (it waits for one running now; that is why calling it from the callback
            // deadlocks) and none starts, so nothing reads `context` any more. It came from
            // `Box::into_raw` in `subscribe` and is freed once, here.
            drop(unsafe { Box::from_raw(self.context) });
        } else {
            // The registration may still call back: `context` is leaked, so it stays valid.
            tracing::debug!(code = result.0, "connectivity hints: cancel failed");
        }
    }
}

/// The callback, on a system thread: a level other than none is a hint. It only sends on an
/// unbounded channel, which never blocks.
unsafe extern "system" fn on_hint(context: *const c_void, hint: NL_NETWORK_CONNECTIVITY_HINT) {
    if context.is_null() || hint.ConnectivityLevel == NetworkConnectivityLevelHintNone {
        return;
    }
    // SAFETY: `context` is the `Hint` registered with this callback, alive until the
    // registration is cancelled, which waits for this call to return (`Registered::drop`).
    // `Hint` is `Sync`, so calls on several threads at once may share it.
    let hint = unsafe { &*context.cast::<Hint>() };
    hint.send();
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
        // SAFETY: the export of that name has `NotifyFn`'s signature (netioapi.h); both are
        // `extern "system"` function pointers of the same size.
        Some(unsafe {
            std::mem::transmute::<unsafe extern "system" fn() -> isize, NotifyFn>(found)
        })
    })
}
