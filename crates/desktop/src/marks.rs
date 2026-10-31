//! The `marks` feature: the speed scoreboard's macOS build. It is the `e2e` build (`e2e.rs`)
//! plus two commands for the page, which plays the scoreboard session itself (`autoplay` in
//! `window.__CLAUSEO_PERF__`) and reports what it measured. It plays only when launched with
//! both `MARKS_OUT` and `E2E_MATTER_DIR` set and not empty (`e2e::init_script`): a `marks`
//! build started by hand shows the app as usual and sends nothing to the person's agent.
//!
//! - `marks_memory`: the app's memory now, in MiB (2^20 bytes): the `phys_footprint` of the app
//!   process plus every `WebKit` process working for it (`marks/macos.rs` says how they are found).
//!   No garbage collection is forced. On other OSes it fails.
//! - `marks_finish { run, error }`: with `error` null, writes `run` (a `RawRun` JSON text,
//!   `ui/e2e/scoreboard/metrics.ts`) to the file named by `MARKS_OUT` and quits the way the app
//!   always quits (host stopped and joined), exit code 0. With `error` set, prints it to stderr,
//!   writes nothing and quits with exit code 1.
//!
//! Contract:
//! - Built only with `--features marks`; `build.rs` defines their permissions and
//!   `e2e::grant_scoreboard` adds `capabilities/marks.json` at setup, in this build only. A
//!   release has neither.
//! - `MARKS_OUT` unset or empty: `marks_finish` fails (`command.invalid`) and the app keeps
//!   running, `error` or not, so a run by hand is never closed under the person.
//! - A `run` that is not JSON, or neither `run` nor `error`, fails the same way; a write that
//!   fails is an `internal` error. Either way nothing quits: the page then reports the failure
//!   with `error`.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use clauseo_protocol::{ErrorCode, HostError};
use tauri::{AppHandle, Runtime};

#[cfg(target_os = "macos")]
mod macos;

/// The file `marks_finish` writes the run to.
const MARKS_OUT_VAR: &str = "MARKS_OUT";

/// The app's memory now, in MiB: its own `phys_footprint` and its `WebKit` processes'.
#[tauri::command]
pub(crate) async fn marks_memory<R: Runtime>(_app: AppHandle<R>) -> Result<f64, HostError> {
    memory_mib()
}

/// Writes `run` to `MARKS_OUT` and quits with 0, or prints `error` and quits with 1.
#[tauri::command]
pub(crate) async fn marks_finish<R: Runtime>(
    app: AppHandle<R>,
    run: Option<String>,
    error: Option<String>,
) -> Result<(), HostError> {
    let out = std::env::var_os(MARKS_OUT_VAR)
        .filter(|out| !out.is_empty())
        .map(PathBuf::from);
    let code = finish(out.as_deref(), run.as_deref(), error.as_deref())?;
    // Through RunEvent::ExitRequested, so `stop_host_then_exit` stops and joins the host first.
    app.exit(code);
    Ok(())
}

/// What `marks_finish` does before it quits: checks its arguments, writes `run` to `out` or
/// prints `error`, and returns the exit code. An error means: do not quit.
fn finish(out: Option<&Path>, run: Option<&str>, error: Option<&str>) -> Result<i32, HostError> {
    let invalid = |message: &str| HostError::new(ErrorCode::CommandInvalid, message);
    let Some(out) = out else {
        return Err(invalid(
            "MARKS_OUT is not set, so the run has nowhere to go; the app keeps running",
        ));
    };
    if let Some(error) = error {
        let mut stderr = std::io::stderr().lock();
        // Nothing else to tell: the exit code says the run failed.
        let _written = writeln!(stderr, "marks run failed: {error}");
        return Ok(1);
    }
    let Some(run) = run else {
        return Err(invalid("marks_finish needs a run or an error"));
    };
    if serde_json::from_str::<serde_json::Value>(run).is_err() {
        return Err(invalid("the run is not JSON"));
    }
    write_run(out, run).map_err(|error| {
        HostError::new(
            ErrorCode::Internal,
            format!("could not write the run to {}: {error}", out.display()),
        )
    })?;
    Ok(0)
}

/// Writes `run` to `out`, replacing what was there.
#[expect(
    clippy::disallowed_methods,
    reason = "marks build only: the run's one file, at the path the scoreboard's runner gives"
)]
fn write_run(out: &Path, run: &str) -> std::io::Result<()> {
    std::fs::write(out, run)
}

/// The app's memory in MiB (macOS).
#[cfg(target_os = "macos")]
fn memory_mib() -> Result<f64, HostError> {
    let bytes = macos::group_footprint().map_err(|error| {
        HostError::new(
            ErrorCode::Internal,
            format!("could not read the memory: {error}"),
        )
    })?;
    // KiB fit a u32 up to 4 TiB, and a u32 converts to f64 exactly.
    let kib = u32::try_from(bytes >> 10).unwrap_or(u32::MAX);
    Ok(f64::from(kib) / 1024.0)
}

/// Not measured outside macOS.
#[cfg(not(target_os = "macos"))]
fn memory_mib() -> Result<f64, HostError> {
    Err(HostError::new(
        ErrorCode::Internal,
        "marks_memory is measured on macOS only",
    ))
}

#[cfg(test)]
mod tests;
