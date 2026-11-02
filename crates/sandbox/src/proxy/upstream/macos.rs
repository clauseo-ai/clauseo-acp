//! The macOS lookup: `CFNetworkCopySystemProxySettings`, then `CFNetworkCopyProxiesForURL`,
//! and for a PAC entry `CFNetworkExecuteProxyAutoConfigurationURL` or `...Script` on a run loop
//! of the calling thread, in a private mode, for at most 5 s. Every CF object this file owns is
//! a `CFRetained`, which releases it when dropped.
#![expect(
    unsafe_code,
    reason = "FFI to CFNetwork's proxy settings and PAC calls"
)]

use std::ffi::c_void;
use std::ptr::NonNull;

use objc2_cf_network::{
    CFNetworkCopyProxiesForURL, CFNetworkCopySystemProxySettings,
    kCFProxyAutoConfigurationJavaScriptKey, kCFProxyAutoConfigurationURLKey, kCFProxyHostNameKey,
    kCFProxyPortNumberKey, kCFProxyTypeAutoConfigurationJavaScript,
    kCFProxyTypeAutoConfigurationURL, kCFProxyTypeHTTP, kCFProxyTypeHTTPS, kCFProxyTypeKey,
    kCFProxyTypeNone,
};
use objc2_core_foundation::{
    CFArray, CFDictionary, CFError, CFNumber, CFRetained, CFRunLoop, CFRunLoopRunResult,
    CFRunLoopSource, CFStreamClientContext, CFString, CFType, CFURL,
};

use super::Route;

/// The longest a PAC run may take, in seconds.
const PAC_TIMEOUT_SECONDS: f64 = 5.0;

/// The run loop mode a PAC run uses, so no other source of the thread runs meanwhile.
const PAC_MODE: &str = "chat.clauseo.sandbox.pac";

/// The port of an HTTP proxy entry with none.
const HTTP_PORT: u16 = 80;

/// The callback `CFNetwork` calls with a PAC result. Declared here, not taken from
/// `objc2-cf-network`, whose type says the list is never null; on an error it is.
type PacCallback = unsafe extern "C-unwind" fn(*mut c_void, *const CFArray, *mut CFError);

#[link(name = "CFNetwork", kind = "framework")]
unsafe extern "C-unwind" {
    fn CFNetworkExecuteProxyAutoConfigurationURL(
        proxy_auto_config_url: &CFURL,
        target_url: &CFURL,
        cb: PacCallback,
        client_context: *mut CFStreamClientContext,
    ) -> *mut CFRunLoopSource;
    fn CFNetworkExecuteProxyAutoConfigurationScript(
        proxy_auto_configuration_script: &CFString,
        target_url: &CFURL,
        cb: PacCallback,
        client_context: *mut CFStreamClientContext,
    ) -> *mut CFRunLoopSource;
}

/// `CFNetwork`'s constant key and type names.
struct Names {
    kind: &'static CFString,
    host: &'static CFString,
    port: &'static CFString,
    pac_url: &'static CFString,
    pac_script: &'static CFString,
    none: &'static CFString,
    http: &'static CFString,
    https: &'static CFString,
    pac_url_kind: &'static CFString,
    pac_script_kind: &'static CFString,
}

fn names() -> Names {
    // SAFETY: these are CFNetwork's exported constant strings, set when the framework loads,
    // before any Rust code runs, and never written after.
    unsafe {
        Names {
            kind: kCFProxyTypeKey,
            host: kCFProxyHostNameKey,
            port: kCFProxyPortNumberKey,
            pac_url: kCFProxyAutoConfigurationURLKey,
            pac_script: kCFProxyAutoConfigurationJavaScriptKey,
            none: kCFProxyTypeNone,
            http: kCFProxyTypeHTTP,
            https: kCFProxyTypeHTTPS,
            pac_url_kind: kCFProxyTypeAutoConfigurationURL,
            pac_script_kind: kCFProxyTypeAutoConfigurationJavaScript,
        }
    }
}

/// The system's route for `target` (`https://host:port/`). `Err` holds a reason with no URL.
pub(super) fn route(target: &str) -> Result<Route, String> {
    let url = cf_url(target)?;
    // SAFETY: takes no arguments and returns an owned dictionary or null.
    let Some(settings) = (unsafe { CFNetworkCopySystemProxySettings() }) else {
        return Err("macOS returned no proxy settings".to_owned());
    };
    // SAFETY: `settings` is the dictionary CFNetwork itself returned, of the key and value types
    // this call expects; `url` is a valid URL.
    let proxies = unsafe { CFNetworkCopyProxiesForURL(&url, &settings) };
    choose(&proxies, Some(&url))
}

/// The route a PAC script gives `target`, run as for an `AutoConfigurationJavaScript` entry.
#[cfg(test)]
pub(super) fn script_route(script: &str, target: &str) -> Result<Route, String> {
    let url = cf_url(target)?;
    run_pac(&Pac::Script(&CFString::from_str(script)), &url)
}

fn cf_url(target: &str) -> Result<CFRetained<CFURL>, String> {
    CFURL::from_string(None, &CFString::from_str(target), None)
        .ok_or_else(|| "macOS refused the target URL".to_owned())
}

/// The first HTTP or HTTPS proxy entry, or a `None` entry (direct), in order. A PAC entry is run
/// when `target` is given (`CFNetwork`'s list) and skipped when not (a PAC run's own result);
/// SOCKS, FTP and unknown entries are skipped. No usable entry: direct.
fn choose(proxies: &CFArray, target: Option<&CFURL>) -> Result<Route, String> {
    let names = names();
    for entry in entries(proxies) {
        let Some(kind) = value::<CFString>(&entry, names.kind) else {
            continue;
        };
        if *kind == *names.none {
            return Ok(Route::Direct);
        }
        if *kind == *names.http || *kind == *names.https {
            if let Some(route) = http_entry(&entry, &names) {
                return Ok(route);
            }
            continue;
        }
        let Some(target) = target else {
            continue;
        };
        if *kind == *names.pac_url_kind {
            let pac = value::<CFURL>(&entry, names.pac_url)
                .ok_or_else(|| "a PAC entry has no URL".to_owned())?;
            return run_pac(&Pac::Url(&pac), target);
        }
        if *kind == *names.pac_script_kind {
            let pac = value::<CFString>(&entry, names.pac_script)
                .ok_or_else(|| "a PAC entry has no script".to_owned())?;
            return run_pac(&Pac::Script(&pac), target);
        }
    }
    Ok(Route::Direct)
}

/// The dictionaries of a proxy list.
fn entries(proxies: &CFArray) -> Vec<CFRetained<CFDictionary<CFString, CFType>>> {
    // SAFETY: CFNetwork's proxy lists (from `CFNetworkCopyProxiesForURL` and a PAC run) hold
    // only dictionaries keyed by strings, whose values are CF objects.
    unsafe { proxies.cast_unchecked::<CFDictionary<CFString, CFType>>() }.to_vec()
}

/// The value of `key` in a proxy entry, when it has the type wanted.
fn value<T: objc2_core_foundation::ConcreteType>(
    entry: &CFDictionary<CFString, CFType>,
    key: &CFString,
) -> Option<CFRetained<T>> {
    entry.get(key)?.downcast::<T>().ok()
}

/// An HTTP or HTTPS proxy entry's host and port (80 when it has none).
fn http_entry(entry: &CFDictionary<CFString, CFType>, names: &Names) -> Option<Route> {
    let host = value::<CFString>(entry, names.host)?.to_string();
    if host.is_empty() {
        return None;
    }
    let port = match value::<CFNumber>(entry, names.port) {
        Some(number) => u16::try_from(number.as_i64()?).ok()?,
        None => HTTP_PORT,
    };
    Some(Route::Http { host, port })
}

/// Where a PAC script comes from.
enum Pac<'a> {
    Url(&'a CFURL),
    Script(&'a CFString),
}

/// What the PAC callback leaves for [`run_pac`].
struct Outcome {
    answer: Option<Result<Route, String>>,
}

/// Called by `CFNetwork`, on the thread running the PAC mode, with the result.
unsafe extern "C-unwind" fn on_pac_result(
    client: *mut c_void,
    proxies: *const CFArray,
    error: *mut CFError,
) {
    // SAFETY: `client` is the `Outcome` `run_pac` put in the context; `run_pac` is still on the
    // stack running the run loop that calls this, and nothing else touches it meanwhile.
    let Some(outcome) = (unsafe { client.cast::<Outcome>().as_mut() }) else {
        return;
    };
    // SAFETY: CFNetwork passes a valid CFError, or null; it outlives the call.
    let error = unsafe { error.as_ref() };
    // SAFETY: CFNetwork passes a valid array, or null on an error; it outlives the call.
    let proxies = unsafe { proxies.as_ref() };
    outcome.answer = Some(match (error, proxies) {
        (Some(error), _) => Err(format!(
            "the PAC script failed ({} {})",
            error
                .domain()
                .map_or_else(String::new, |domain| domain.to_string()),
            error.code()
        )),
        (None, Some(proxies)) => choose(proxies, None),
        (None, None) => Err("the PAC script gave no result".to_owned()),
    });
    if let Some(run_loop) = CFRunLoop::current() {
        run_loop.stop();
    }
}

/// Runs a PAC script for `target` on this thread's run loop, in [`PAC_MODE`], for at most
/// [`PAC_TIMEOUT_SECONDS`].
fn run_pac(pac: &Pac<'_>, target: &CFURL) -> Result<Route, String> {
    let run_loop = CFRunLoop::current().ok_or_else(|| "this thread has no run loop".to_owned())?;
    let mut outcome = Outcome { answer: None };
    let mut context = CFStreamClientContext {
        version: 0,
        info: (&raw mut outcome).cast(),
        retain: None,
        release: None,
        copyDescription: None,
    };
    let source = match pac {
        // SAFETY: the URLs are valid; the callback has the signature CFNetwork calls; the
        // context points to `outcome`, which outlives the source: it is invalidated below,
        // before `outcome` goes out of scope, and CFNetwork copies the context itself.
        Pac::Url(pac) => unsafe {
            CFNetworkExecuteProxyAutoConfigurationURL(pac, target, on_pac_result, &raw mut context)
        },
        // SAFETY: as above, with a valid script string.
        Pac::Script(script) => unsafe {
            CFNetworkExecuteProxyAutoConfigurationScript(
                script,
                target,
                on_pac_result,
                &raw mut context,
            )
        },
    };
    let source =
        NonNull::new(source).ok_or_else(|| "CFNetwork did not start the PAC run".to_owned())?;
    // SAFETY: the Execute calls return a run loop source the caller owns (+1), as Apple's
    // headers mark it and objc2-cf-network's own binding treats it.
    let source = unsafe { CFRetained::from_raw(source) };
    let mode = CFString::from_static_str(PAC_MODE);
    run_loop.add_source(Some(&source), Some(&mode));
    let ran = CFRunLoop::run_in_mode(Some(&mode), PAC_TIMEOUT_SECONDS, false);
    run_loop.remove_source(Some(&source), Some(&mode));
    source.invalidate();
    outcome.answer.unwrap_or_else(|| {
        Err(if ran == CFRunLoopRunResult::TimedOut {
            "the PAC run took over 5 s".to_owned()
        } else {
            "the PAC run ended with no result".to_owned()
        })
    })
}
