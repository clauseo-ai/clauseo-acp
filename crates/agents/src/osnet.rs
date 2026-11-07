//! Whether the OS itself reports no network at all, for the install's offline wait: while it
//! does, a job that has met no network for three tries in a row still says offline, never
//! unreachable (`install::Job::waiting_reason`), so a lawyer on a train is not sent to IT.
//!
//! Windows asks the Network List Manager (`INetworkListManager::GetConnectivity`, disconnected
//! when it reports no IPv4 or IPv6 connectivity at all). Other OSes report nothing here (never
//! disconnected): macOS's reachability API is not among this crate's dependencies, and there the
//! unreachable words ("if your internet is working") cover it.

#[cfg(windows)]
mod windows;

/// The OS's view of the network. A trait so tests can fake "no network".
pub trait OsNetwork: Send + Sync {
    /// Whether the OS reports no network connectivity at all. `false` when it cannot tell.
    fn disconnected(&self) -> bool;
}

/// The OS's own answer: the Network List Manager on Windows, never disconnected elsewhere.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemNetwork;

impl OsNetwork for SystemNetwork {
    fn disconnected(&self) -> bool {
        #[cfg(windows)]
        {
            windows::disconnected().unwrap_or(false)
        }
        #[cfg(not(windows))]
        {
            false
        }
    }
}
