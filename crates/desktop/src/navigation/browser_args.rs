//! The Chromium switches `WebView2` starts with on Windows: wry's own, and no host name lookups.
//!
//! Contract:
//! - The navigation guard (`navigation.rs`) refuses a navigation only after `WebView2` has started
//!   it, and the CSP does not cover `<link rel=dns-prefetch>` or `preconnect`; on a Windows VM
//!   `msedgewebview2.exe` still sent DNS queries for refused navigation targets and for those
//!   links' hosts (DNS-Client events 3006 and 3010). A host name's labels can carry data.
//!   [`browser_args`] adds [`NO_DNS`], which maps every host name to "not found" inside the
//!   browser's own resolver, so no name reaches the OS resolver or the network.
//! - The app needs no name resolved: its pages come from `https://tauri.localhost`, which
//!   `WebView2` hands to Tauri's interception before any lookup, `clauseo-file:` and `ipc:` are
//!   custom schemes, and every request to the network (sign-in, updates, agents, `open_external`)
//!   is made by Rust, outside the webview. The rules exclude no host. A development build loads
//!   `devUrl` (`http://localhost:1420`), which would then fail too, so there only that host is
//!   excluded ([`browser_args`] gets it from `navigation::dev_url`, `None` in any build made with
//!   `custom-protocol`).
//! - Passing arguments replaces wry's defaults (wry 0.57.0 `src/webview2/mod.rs`: the
//!   `--disable-features` list, plus autoplay and proxy switches the app never asks for), so
//!   [`WRY_DEFAULT_ARGS`] is passed again first.
//! - Both windows share one `WebView2` environment, whose arguments must match: [`apply`], which
//!   `navigation::guard` calls for both, gives each the same string, the e2e build's debugging
//!   switches included.
//! - `WebView2` lets a user- or machine-set `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS` take the
//!   place of these arguments, which would drop the rule. [`keep_no_dns_in_env`], first thing in
//!   `main`, appends [`NO_DNS`] to that variable when it is set, keeping the flags it has (support
//!   and debugging flags, usually). It goes last even after `--host-resolver-rules` of the
//!   variable's own, since Chromium takes the last value of a repeated switch; only a value that
//!   already ends with it (a relaunch, a child) is left as it is. A variable set by whatever starts
//!   the process after this ran is not covered; in a development build the appended rule has no
//!   `devUrl` exclusion, so a set variable there stops the dev page loading.
//! - `WebView2` also takes `AdditionalBrowserArguments` policy values, under `HKCU` or `HKLM`
//!   `Software\Policies\Microsoft\Edge\WebView2\AdditionalBrowserArguments`, named by the exe,
//!   in place of these arguments. Only something already running with the user's rights, or an
//!   admin, can set one; [`apply`] logs one warning naming the hive when one is set
//!   ([`registry_warning`]), never the value.

use std::ffi::{OsStr, OsString};

use tauri::Url;

#[cfg(windows)]
use tauri::{Manager, Runtime, WebviewWindowBuilder};
#[cfg(windows)]
use windows::Win32::Foundation::ERROR_SUCCESS;
#[cfg(windows)]
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_ANY, RegGetValueW,
};
#[cfg(windows)]
use windows::core::{PCWSTR, w};

#[cfg(test)]
mod tests;

/// wry 0.57.0's own arguments when none are passed (`src/webview2/mod.rs`): the mini menu, the
/// PDF menu and `SmartScreen` off.
pub(crate) const WRY_DEFAULT_ARGS: &str =
    "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection";

/// Every host name resolves to "not found" without a lookup. `WebView2` splits the argument
/// string like a command line, so the value, which has spaces, is quoted.
pub(crate) const NO_DNS: &str = r#"--host-resolver-rules="MAP * ~NOTFOUND""#;

/// The arguments for every window: [`WRY_DEFAULT_ARGS`] and [`NO_DNS`], with `dev`'s host
/// excluded from the rules when there is a development `devUrl`.
pub(crate) fn browser_args(dev: Option<&Url>) -> String {
    match dev.and_then(Url::host_str) {
        Some(host) => {
            format!(r#"{WRY_DEFAULT_ARGS} --host-resolver-rules="MAP * ~NOTFOUND, EXCLUDE {host}""#)
        }
        None => format!("{WRY_DEFAULT_ARGS} {NO_DNS}"),
    }
}

/// `builder` with [`browser_args`], and in the e2e build the debugging switches after them.
#[cfg(windows)]
pub(crate) fn apply<'a, R: Runtime, M: Manager<R>>(
    builder: WebviewWindowBuilder<'a, R, M>,
    app: &M,
) -> WebviewWindowBuilder<'a, R, M> {
    static CHECKED: std::sync::Once = std::sync::Once::new();
    CHECKED.call_once(|| {
        if let Some(hives) = registry_warning(policy_set(HKEY_CURRENT_USER), policy_set(HKEY_LOCAL_MACHINE)) {
            tracing::warn!(hives, "a WebView2 AdditionalBrowserArguments policy is set for this exe: it replaces the arguments that stop DNS lookups");
        }
    });
    let args = browser_args(super::dev_url(app.config().build.dev_url.as_ref()));
    #[cfg(feature = "e2e")]
    let args = args + &crate::e2e::debugging_args();
    builder.additional_browser_args(&args)
}

/// Which hives to name in the policy warning, given whether `HKCU` and `HKLM` have a value.
pub(crate) fn registry_warning(hkcu: bool, hklm: bool) -> Option<&'static str> {
    match (hkcu, hklm) {
        (true, true) => Some("HKCU, HKLM"),
        (true, false) => Some("HKCU"),
        (false, true) => Some("HKLM"),
        (false, false) => None,
    }
}

/// Whether `hive` has an `AdditionalBrowserArguments` policy value named by this exe's file name.
/// A failed read counts as none.
#[cfg(windows)]
#[expect(unsafe_code, reason = "FFI to the Win32 registry")]
fn policy_set(hive: HKEY) -> bool {
    let Some(name) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.file_name().map(OsStr::to_os_string))
    else {
        return false;
    };
    let name: Vec<u16> = std::os::windows::ffi::OsStrExt::encode_wide(name.as_os_str())
        .chain([0])
        .collect();
    // SAFETY: the subkey is a NUL-terminated literal and `name` a NUL-terminated buffer alive for
    // the call; with no type, data or size out-pointers it only reports whether the value exists.
    let read = unsafe {
        RegGetValueW(
            hive,
            w!("Software\\Policies\\Microsoft\\Edge\\WebView2\\AdditionalBrowserArguments"),
            PCWSTR(name.as_ptr()),
            RRF_RT_ANY,
            None,
            None,
            None,
        )
    };
    read == ERROR_SUCCESS
}

/// The variable through which `WebView2` takes arguments in place of the app's.
pub(crate) const ENV_OVERRIDE: &str = "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS";

/// The value [`ENV_OVERRIDE`] should have, given its value `current`: `None` to leave it (unset,
/// or already ending with [`NO_DNS`]), else `current` with [`NO_DNS`] after a space, last, so it
/// wins over any earlier `--host-resolver-rules`.
pub(crate) fn env_override_with_rule(current: Option<&OsStr>) -> Option<OsString> {
    let current = current?;
    if current.to_string_lossy().trim_end().ends_with(NO_DNS) {
        return None;
    }
    let mut value = current.to_os_string();
    if !current.is_empty() {
        value.push(" ");
    }
    value.push(NO_DNS);
    Some(value)
}

/// Appends [`NO_DNS`] to a set [`ENV_OVERRIDE`] that does not already end with it. `main` calls it
/// first, before any thread starts.
#[cfg(windows)]
#[expect(
    unsafe_code,
    reason = "set_var, which is unsafe once other threads may read the environment"
)]
pub fn keep_no_dns_in_env() {
    if let Some(value) = env_override_with_rule(std::env::var_os(ENV_OVERRIDE).as_deref()) {
        // SAFETY: `main` calls this as its first statement, while the process has one thread
        // and nothing has read the environment: no Tauri, logging, runtime or other thread yet.
        unsafe { std::env::set_var(ENV_OVERRIDE, value) };
    }
}
