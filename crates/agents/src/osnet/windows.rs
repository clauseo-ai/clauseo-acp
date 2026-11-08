//! The Network List Manager's connectivity (`INetworkListManager::GetConnectivity`), through
//! COM on the calling thread.
#![expect(unsafe_code, reason = "FFI to COM's Network List Manager")]

use windows::Win32::Networking::NetworkListManager::{
    INetworkListManager, NLM_CONNECTIVITY_DISCONNECTED, NetworkListManager,
};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};

/// Whether the Network List Manager reports no connectivity at all; `None` when it cannot be
/// asked.
pub(super) fn disconnected() -> Option<bool> {
    // SAFETY: a plain call; the reserved pointer is null as required. Balanced below when it
    // succeeded (`S_OK` or `S_FALSE`); a thread already in another apartment gets an error and
    // uses its own.
    let initialised = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
    let asked = ask();
    if initialised {
        // SAFETY: pairs the successful `CoInitializeEx` above, on the same thread; `ask`'s
        // interface has been released.
        unsafe { CoUninitialize() };
    }
    asked
}

fn ask() -> Option<bool> {
    // SAFETY: the class and interface ids come from the bindings; no outer object.
    let manager: INetworkListManager =
        unsafe { CoCreateInstance(&NetworkListManager, None, CLSCTX_ALL) }.ok()?;
    // SAFETY: `manager` is a live interface; the call only reads.
    let connectivity = unsafe { manager.GetConnectivity() }.ok()?;
    Some(connectivity == NLM_CONNECTIVITY_DISCONNECTED)
}
