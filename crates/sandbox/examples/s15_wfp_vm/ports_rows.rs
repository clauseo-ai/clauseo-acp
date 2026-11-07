//! `ports` (S15 PR7 re-check, item 8): tier B's port check as the VM sees it. Run it as the
//! desktop user, unelevated, since `check_launch` runs in the lawyer's process. Prints every
//! `MSFT_NetTCPSetting` dynamic range WMI reports and how long the read took, then the whole
//! check (`wfp::check_ports`, listeners included) and its time. For the IPv6 row, set the
//! IPv6 range apart first and put it back after:
//!
//! ```text
//! netsh int ipv6 set dynamicport tcp start=40000 num=10000   (elevated; overlaps 46100-46199)
//! s15_wfp_vm.exe ports                                        (does WMI report 40000-49999?)
//! netsh int ipv6 set dynamicport tcp start=49152 num=16384   (elevated; the default again)
//! ```

use std::time::{Duration, Instant};

use clauseo_sandbox::windows::wfp::{check_ports, dynamic_port_ranges};

use super::say;

/// Runs `f` and how long it took.
#[expect(
    clippy::disallowed_methods,
    reason = "the VM row measures the WMI read's cost on a real machine"
)]
fn timed<T>(f: impl FnOnce() -> T) -> (T, Duration) {
    let started = Instant::now();
    let value = f();
    (value, started.elapsed())
}

pub(crate) fn ports() {
    let (ranges, read) = timed(dynamic_port_ranges);
    say(&serde_json::json!({
        "row": "WMI's dynamic port ranges (every MSFT_NetTCPSetting)",
        "got": format!("{ranges:?}"),
        "ms": read.as_millis(),
    })
    .to_string());
    let (checked, whole) = timed(|| check_ports(true));
    say(&serde_json::json!({
        "row": "the whole port check (dynamic, excluded, listeners)",
        "got": format!("{checked:?}"),
        "ms": whole.as_millis(),
    })
    .to_string());
}
