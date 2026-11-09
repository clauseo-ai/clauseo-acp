//! A probe the jail tests run inside a profile: this test binary, copied to where the profile
//! can run it, started with `CLAUSEO_JAIL_PROBE` set. Without the variable the test does
//! nothing. Each answer is one stdout line that starts `PROBE `.
//!
//! - `marker <write path> <read path>`: `sandboxed`, and whether `file-write-data` on the first
//!   path and `file-read-data` on the second are allowed (`sandbox_check`, the guard's marker).
//! - `nest`: whether the browser launcher's outermost check refuses here.
//! - `map <path>`: loads the library with `dlopen`, which maps it as code, or names the error.
//! - `lookup <name>`: looks up the Mach service, and prints `bootstrap_look_up`'s result (0 when
//!   found; 1100, `BOOTSTRAP_NOT_PRIVILEGED`, when the profile refuses it).
//! - `bind <path>`: binds a Unix socket there, or names the error.

#![expect(
    clippy::disallowed_methods,
    reason = "the probe copies itself and a dylib into the test world; clippy.toml bans writes"
)]

use std::ffi::{CString, c_char, c_int};
use std::path::{Path, PathBuf};

use clauseo_sandbox::macos::browser::{path_allowed, refuse_nesting, sandboxed};

/// The variable that turns the probe on, and its two arguments.
const MODE: &str = "CLAUSEO_JAIL_PROBE";
const ARG1: &str = "CLAUSEO_JAIL_PROBE_1";
const ARG2: &str = "CLAUSEO_JAIL_PROBE_2";

/// The probe's arguments to the test harness: this one test, its output shown.
pub(crate) const ARGS: [&str; 4] = ["probe::probe", "--exact", "--nocapture", "--test-threads=1"];

#[test]
fn probe() {
    let Ok(mode) = std::env::var(MODE) else {
        return;
    };
    let arg = |name| PathBuf::from(std::env::var(name).unwrap_or_default());
    match mode.as_str() {
        "marker" => println!(
            "PROBE sandboxed={} write={} read={}",
            sandboxed(),
            path_allowed(0, "file-write-data", &arg(ARG1)),
            path_allowed(0, "file-read-data", &arg(ARG2))
        ),
        "nest" => println!("PROBE nest={:?}", refuse_nesting().err()),
        "map" => println!("PROBE map={}", map(&arg(ARG1))),
        "lookup" => println!(
            "PROBE lookup={}",
            lookup(&std::env::var(ARG1).unwrap_or_default())
        ),
        "bind" => match std::os::unix::net::UnixListener::bind(arg(ARG1)) {
            Ok(_) => println!("PROBE bind=ok"),
            Err(error) => println!("PROBE bind={error}"),
        },
        other => println!("PROBE unknown mode {other}"),
    }
}

/// Loads `path` with `dlopen`: `ok`, or dyld's error.
#[expect(
    unsafe_code,
    reason = "dlopen and dlerror, to ask the profile for file-map-executable"
)]
pub(crate) fn map(path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    unsafe extern "C" {
        fn dlopen(path: *const c_char, mode: c_int) -> *mut std::ffi::c_void;
        fn dlerror() -> *const c_char;
    }
    const RTLD_NOW: c_int = 2;
    let Ok(path) = CString::new(path.as_os_str().as_bytes()) else {
        return "a NUL in the path".to_owned();
    };
    // SAFETY: path is NUL-terminated and outlives the call; dlerror's string is read at once.
    unsafe {
        if dlopen(path.as_ptr(), RTLD_NOW).is_null() {
            let error = dlerror();
            if error.is_null() {
                return "dlopen failed".to_owned();
            }
            std::ffi::CStr::from_ptr(error)
                .to_string_lossy()
                .into_owned()
        } else {
            "ok".to_owned()
        }
    }
}

/// Looks up the Mach service `name` from this process: `bootstrap_look_up`'s result.
#[expect(
    unsafe_code,
    reason = "bootstrap_look_up, to ask the profile for mach-lookup"
)]
pub(crate) fn lookup(name: &str) -> i32 {
    unsafe extern "C" {
        static bootstrap_port: u32;
        fn bootstrap_look_up(bp: u32, name: *const c_char, sp: *mut u32) -> c_int;
    }
    let Ok(name) = CString::new(name) else {
        return -1;
    };
    let mut port = 0_u32;
    // SAFETY: bootstrap_port is libSystem's, set at start; name is NUL-terminated; port is a
    // valid out-pointer. A port received is left to the process's exit.
    unsafe { bootstrap_look_up(bootstrap_port, name.as_ptr(), &raw mut port) }
}

/// A library to load, copied to `to`: a system dylib that is a file on disk (most live only
/// in the shared cache). The folder must exist.
pub(crate) fn dylib_to(to: &Path) -> PathBuf {
    let from = ["/usr/lib/libLeaksAtExit.dylib", "/usr/lib/libgmalloc.dylib"]
        .into_iter()
        .map(Path::new)
        .find(|p| p.exists())
        .expect("a system dylib on disk");
    std::fs::copy(from, to).unwrap();
    to.to_owned()
}

/// The probe copied to `to`, keeping its execute bit; the folder must exist.
pub(crate) fn copy_to(to: &Path) -> PathBuf {
    std::fs::copy(std::env::current_exe().unwrap(), to).unwrap();
    to.to_owned()
}

/// The variables that start the probe in `mode` with `args`.
pub(crate) fn env<'a>(mode: &'a str, args: &[&'a str]) -> Vec<(&'a str, &'a str)> {
    let mut env = vec![(MODE, mode)];
    for (name, value) in [ARG1, ARG2].into_iter().zip(args) {
        env.push((name, *value));
    }
    env
}

/// The probe's answer line in `stdout`, without `PROBE `.
#[track_caller]
pub(crate) fn answer(stdout: &str) -> String {
    stdout
        .lines()
        .find_map(|line| line.split_once("PROBE ").map(|(_, answer)| answer))
        .unwrap_or_else(|| panic!("no probe answer in {stdout:?}"))
        .to_owned()
}
