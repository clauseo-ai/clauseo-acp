//! The Windows lookup: `WinHttpGetIEProxyConfigForCurrentUser`; when auto-detect is on or an
//! auto-config URL is set, `WinHttpGetProxyForUrl` (WPAD by DHCP and DNS, then the URL); else
//! the static proxy string with its bypass list. Every string `WinHTTP` returns is freed with
//! `GlobalFree`, and the session handle is closed, when its owner drops.
#![expect(
    unsafe_code,
    reason = "FFI to WinHTTP's proxy configuration and PAC calls"
)]

use windows::Win32::Foundation::{GlobalFree, HGLOBAL};
use windows::Win32::Networking::WinHttp::{
    ERROR_WINHTTP_AUTODETECTION_FAILED, WINHTTP_ACCESS_TYPE_NAMED_PROXY,
    WINHTTP_ACCESS_TYPE_NO_PROXY, WINHTTP_AUTO_DETECT_TYPE_DHCP, WINHTTP_AUTO_DETECT_TYPE_DNS_A,
    WINHTTP_AUTOPROXY_AUTO_DETECT, WINHTTP_AUTOPROXY_CONFIG_URL, WINHTTP_AUTOPROXY_OPTIONS,
    WINHTTP_AUTOPROXY_RUN_INPROCESS, WINHTTP_CURRENT_USER_IE_PROXY_CONFIG, WINHTTP_PROXY_INFO,
    WinHttpCloseHandle, WinHttpGetIEProxyConfigForCurrentUser, WinHttpGetProxyForUrl, WinHttpOpen,
    WinHttpSetTimeouts,
};
use windows::core::{BOOL, HRESULT, HSTRING, PCWSTR, PWSTR, w};

use super::Route;
use super::parse::{self, Bypass};

/// `WinHTTP`'s resolve, connect, send and receive timeouts for a PAC download, in milliseconds.
const PAC_TIMEOUT_MS: i32 = 5_000;

/// `ERROR_WINHTTP_UNABLE_TO_DOWNLOAD_SCRIPT`.
const UNABLE_TO_DOWNLOAD_SCRIPT: u32 = 12_167;

/// `ERROR_WINHTTP_AUTOPROXY_SERVICE_ERROR`.
const AUTOPROXY_SERVICE_ERROR: u32 = 12_178;

/// The system's route for `target` (`https://host:port/`). `Err` holds a reason with no URL.
pub(super) fn route(target: &str, host: &str, port: u16) -> Result<Route, String> {
    let config = IeConfig::current()?;
    let auto_config_url = config.auto_config_url.take_string();
    if config.auto_detect || auto_config_url.is_some() {
        let session = Session::open()?;
        if config.auto_detect {
            match session.proxy_for_url(target, &Pac::AutoDetect) {
                Ok(route) => return Ok(route),
                Err(error) if error == ERROR_WINHTTP_AUTODETECTION_FAILED => {
                    tracing::debug!(host, port, "no WPAD file found");
                }
                Err(error) => {
                    tracing::warn!(host, port, error, "WPAD failed; trying the next setting");
                }
            }
        }
        if let Some(url) = auto_config_url {
            return session
                .proxy_for_url(target, &Pac::Url(&url))
                .map_err(|error| format!("the PAC file failed (WinHTTP error {error})"));
        }
    }
    let Some(proxy) = config.proxy.take_string() else {
        return Ok(Route::Direct);
    };
    let bypass = Bypass::parse(&config.bypass.take_string().unwrap_or_default());
    if bypass.matches(&parse::normal_host(host), port) {
        return Ok(Route::Direct);
    }
    Ok(parse::winhttp_proxy(&proxy).unwrap_or(Route::Direct))
}

/// A string `WinHTTP` allocated, freed with `GlobalFree` on drop.
struct WinString(PWSTR);

impl WinString {
    /// The text, or `None` for a null or empty string.
    fn take_string(&self) -> Option<String> {
        if self.0.is_null() {
            return None;
        }
        // SAFETY: a non-null string from WinHTTP is NUL-terminated UTF-16 that lives until
        // this owner frees it.
        let text = unsafe { self.0.to_string() }.ok()?;
        (!text.trim().is_empty()).then_some(text)
    }
}

impl Drop for WinString {
    fn drop(&mut self) {
        if self.0.is_null() {
            return;
        }
        // SAFETY: WinHTTP allocated this string with GlobalAlloc and handed it to the caller,
        // which frees it once; nothing reads it after.
        let freed = unsafe { GlobalFree(Some(HGLOBAL(self.0.0.cast()))) };
        // GlobalFree returns null on success, which the binding reports as an error with code 0.
        if let Err(error) = freed
            && error.code().is_err()
        {
            tracing::debug!(
                code = error.code().0,
                "GlobalFree failed on a WinHTTP string"
            );
        }
    }
}

/// The current user's proxy settings.
struct IeConfig {
    auto_detect: bool,
    auto_config_url: WinString,
    proxy: WinString,
    bypass: WinString,
}

impl IeConfig {
    fn current() -> Result<IeConfig, String> {
        let mut raw = WINHTTP_CURRENT_USER_IE_PROXY_CONFIG::default();
        // SAFETY: `raw` is a valid, writable struct; on success WinHTTP fills it with strings
        // the caller owns, which `WinString` frees.
        unsafe { WinHttpGetIEProxyConfigForCurrentUser(&raw mut raw) }.map_err(|error| {
            format!(
                "WinHttpGetIEProxyConfigForCurrentUser failed ({})",
                error.code().0
            )
        })?;
        Ok(IeConfig {
            auto_detect: raw.fAutoDetect.as_bool(),
            auto_config_url: WinString(raw.lpszAutoConfigUrl),
            proxy: WinString(raw.lpszProxy),
            bypass: WinString(raw.lpszProxyBypass),
        })
    }
}

/// Where a PAC file comes from.
pub(super) enum Pac<'a> {
    /// WPAD, by DHCP and DNS.
    AutoDetect,
    /// A PAC URL.
    Url(&'a str),
}

/// A `WinHTTP` session, closed on drop.
pub(super) struct Session(*mut core::ffi::c_void);

impl Session {
    pub(super) fn open() -> Result<Session, String> {
        // SAFETY: the agent name is a static NUL-terminated string; the proxy and bypass names
        // are null, as WINHTTP_ACCESS_TYPE_NO_PROXY requires.
        let handle = unsafe {
            WinHttpOpen(
                w!("Clauseo"),
                WINHTTP_ACCESS_TYPE_NO_PROXY,
                PCWSTR::null(),
                PCWSTR::null(),
                0,
            )
        };
        if handle.is_null() {
            return Err(format!(
                "WinHttpOpen failed ({})",
                windows::core::Error::from_thread().code().0
            ));
        }
        let session = Session(handle);
        // SAFETY: `session.0` is the open session handle.
        let timeouts = unsafe {
            WinHttpSetTimeouts(
                session.0,
                PAC_TIMEOUT_MS,
                PAC_TIMEOUT_MS,
                PAC_TIMEOUT_MS,
                PAC_TIMEOUT_MS,
            )
        };
        if let Err(error) = timeouts {
            tracing::debug!(code = error.code().0, "WinHttpSetTimeouts failed");
        }
        Ok(session)
    }

    /// The route the PAC file gives `target`. `Err` holds the Win32 error code. A PAC URL the
    /// `WinHTTP` service cannot fetch is tried once more in this process.
    pub(super) fn proxy_for_url(&self, target: &str, pac: &Pac<'_>) -> Result<Route, u32> {
        match self.run(target, pac, 0) {
            Err(error)
                if matches!(pac, Pac::Url(_))
                    && (error == UNABLE_TO_DOWNLOAD_SCRIPT || error == AUTOPROXY_SERVICE_ERROR) =>
            {
                self.run(target, pac, WINHTTP_AUTOPROXY_RUN_INPROCESS)
            }
            answer => answer,
        }
    }

    fn run(&self, target: &str, pac: &Pac<'_>, extra_flags: u32) -> Result<Route, u32> {
        let config_url;
        let mut options = WINHTTP_AUTOPROXY_OPTIONS {
            fAutoLogonIfChallenged: BOOL::from(true),
            ..WINHTTP_AUTOPROXY_OPTIONS::default()
        };
        match pac {
            Pac::AutoDetect => {
                options.dwFlags = WINHTTP_AUTOPROXY_AUTO_DETECT | extra_flags;
                options.dwAutoDetectFlags =
                    WINHTTP_AUTO_DETECT_TYPE_DHCP | WINHTTP_AUTO_DETECT_TYPE_DNS_A;
            }
            Pac::Url(url) => {
                config_url = HSTRING::from(*url);
                options.dwFlags = WINHTTP_AUTOPROXY_CONFIG_URL | extra_flags;
                options.lpszAutoConfigUrl = PCWSTR(config_url.as_ptr());
            }
        }
        let target = HSTRING::from(target);
        let mut info = WINHTTP_PROXY_INFO::default();
        // SAFETY: `self.0` is an open session; `target` and `config_url` are NUL-terminated
        // strings that outlive the call; `options` and `info` are valid and writable. On
        // success WinHTTP fills `info` with strings the caller owns, freed below.
        let done =
            unsafe { WinHttpGetProxyForUrl(self.0, &target, &raw mut options, &raw mut info) };
        let proxy = WinString(info.lpszProxy);
        let _bypass_freed_on_drop = WinString(info.lpszProxyBypass);
        done.map_err(|error| win32_code(error.code()))?;
        if info.dwAccessType != WINHTTP_ACCESS_TYPE_NAMED_PROXY {
            return Ok(Route::Direct);
        }
        let Some(text) = proxy.take_string() else {
            return Ok(Route::Direct);
        };
        Ok(if parse::is_pac_form(&text) {
            parse::pac_result(&text)
        } else {
            parse::winhttp_proxy(&text).unwrap_or(Route::Direct)
        })
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // SAFETY: `self.0` is the session handle `WinHttpOpen` returned, closed once, here.
        if let Err(error) = unsafe { WinHttpCloseHandle(self.0) } {
            tracing::debug!(code = error.code().0, "WinHttpCloseHandle failed");
        }
    }
}

/// The Win32 error code inside an `HRESULT_FROM_WIN32` value, or the raw value.
fn win32_code(code: HRESULT) -> u32 {
    let bits = code.0.cast_unsigned();
    if bits & 0xFFFF_0000 == 0x8007_0000 {
        bits & 0xFFFF
    } else {
        bits
    }
}
