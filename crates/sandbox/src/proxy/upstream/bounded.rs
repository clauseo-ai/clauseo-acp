//! The bounded thread a Windows lookup runs on: WPAD's DHCP and DNS discovery do not all honour
//! `WinHTTP`'s timeouts.

use std::time::Duration;

/// Runs `lookup` on its own thread and waits at most `limit` for it. A lookup still running
/// then is left to finish on its thread, and its answer is dropped. `threads` counts the
/// threads alive, those left running included; past `most` no thread starts and the lookup
/// fails, so hung lookups cannot pile up threads.
pub(super) fn bounded<T: Send + 'static>(
    limit: Duration,
    threads: &'static std::sync::atomic::AtomicUsize,
    most: usize,
    lookup: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    use std::sync::atomic::Ordering;
    threads
        .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
            (n < most).then_some(n + 1)
        })
        .map_err(|_running| format!("{most} lookups are still running"))?;
    let (answer, answered) = std::sync::mpsc::sync_channel(1);
    let spawned = std::thread::Builder::new()
        .name("clauseo-proxy-lookup".to_owned())
        .spawn(move || {
            // Gives the slot back when the thread ends, a panicking lookup included.
            let _slot = Release(threads);
            let route = lookup();
            // The waiter may have given up; its answer is then dropped.
            drop(answer.send(route));
        });
    if let Err(error) = spawned {
        threads.fetch_sub(1, Ordering::SeqCst);
        return Err(format!("could not start the lookup thread ({error})"));
    }
    answered
        .recv_timeout(limit)
        .map_err(|_timeout| format!("the lookup took over {} s", limit.as_secs()))?
}

/// One running lookup thread, counted in `bounded`'s counter until dropped.
struct Release(&'static std::sync::atomic::AtomicUsize);

impl Drop for Release {
    fn drop(&mut self) {
        self.0.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}
