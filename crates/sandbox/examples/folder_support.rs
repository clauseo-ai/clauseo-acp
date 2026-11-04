//! Times `folder_support` on each folder the arguments name, for the measurement on a Windows VM:
//!
//! ```text
//! cargo run -p clauseo-sandbox --example folder_support -- <folder>...
//! ```
//!
//! For each folder it writes one JSON line on stdout: the result, then after 10 warm-up calls the
//! median, p95 and max of 1000 calls, in microseconds. Off Windows it says so and does nothing.

use std::error::Error;

#[cfg(windows)]
use std::io::{self, Write};
#[cfg(windows)]
use std::path::{Path, PathBuf};
#[cfg(windows)]
use std::time::Instant;

#[cfg(windows)]
use clauseo_sandbox::windows::{FolderSupport, folder_support};

/// Calls made before the timed ones.
#[cfg(windows)]
const WARM_UP: usize = 10;

/// Timed calls per folder.
#[cfg(windows)]
const CALLS: usize = 1000;

/// What one folder's timing came to.
#[cfg(windows)]
struct Timing {
    result: FolderSupport,
    median: f64,
    p95: f64,
    max: f64,
}

#[cfg(windows)]
fn main() -> Result<(), Box<dyn Error>> {
    let paths: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    if paths.is_empty() {
        return Err("usage: folder_support <folder>...".into());
    }
    let mut out = io::stdout().lock();
    for path in &paths {
        let line = match measure(path) {
            Ok(timing) => serde_json::json!({
                "path": path.display().to_string(),
                "result": format!("{:?}", timing.result),
                "median_us": timing.median,
                "p95_us": timing.p95,
                "max_us": timing.max,
            }),
            Err(error) => serde_json::json!({
                "path": path.display().to_string(),
                "error": error.to_string(),
            }),
        };
        writeln!(out, "{line}")?;
    }
    Ok(())
}

/// The folder's answer, then the timing of `CALLS` calls after `WARM_UP` warm-up calls.
#[cfg(windows)]
#[expect(
    clippy::disallowed_methods,
    reason = "the example times each call: the wall clock is what it measures"
)]
fn measure(path: &Path) -> io::Result<Timing> {
    let result = folder_support(path)?;
    for _ in 0..WARM_UP {
        folder_support(path)?;
    }
    let mut micros = Vec::with_capacity(CALLS);
    for _ in 0..CALLS {
        let start = Instant::now();
        folder_support(path)?;
        micros.push(start.elapsed().as_secs_f64() * 1e6);
    }
    micros.sort_by(f64::total_cmp);
    let last = micros.len() - 1;
    Ok(Timing {
        result,
        median: micros[last / 2],
        p95: micros[last * 95 / 100],
        max: micros[last],
    })
}

#[cfg(not(windows))]
fn main() -> Result<(), Box<dyn Error>> {
    use std::io::Write as _;

    writeln!(
        std::io::stdout().lock(),
        "folder_support: Windows only; this OS has no folder check to time"
    )?;
    Ok(())
}
