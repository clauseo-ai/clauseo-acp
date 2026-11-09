//! The zip's route: the browser pins fetch's (`network.upstream_proxy`, else the system's
//! proxy), given to curl on its command line, and a refused setting fails the download rather
//! than go around it.

use std::ffi::OsString;
use std::path::Path;

use clauseo_protocol::BrowserInstallFailure;
use clauseo_sandbox::{Route, Upstream};

use super::support::Fake;
use super::{job, run_with};
use crate::browser::chrome::Platform;
use crate::browser::install::Cancel;
use crate::browser::install::system::download_args;
use crate::pipeline::tests::support::TempDir;

/// The arguments for `route`, the rest fixed.
fn args(route: &Route) -> Vec<OsString> {
    download_args(
        "https://storage.googleapis.com/z.zip",
        route,
        Path::new("/w/headers.txt"),
        Path::new("/w/chrome.zip"),
        1234,
    )
}

/// The value after `flag` in `args`, if `flag` is there once.
fn after(args: &[OsString], flag: &str) -> Option<OsString> {
    let at: Vec<usize> = (0..args.len()).filter(|&at| args[at] == flag).collect();
    match at.as_slice() {
        [at] => args.get(at + 1).cloned(),
        _ => None,
    }
}

#[test]
fn the_proxy_reaches_curl_on_its_command_line() {
    let proxy = Route::Http {
        host: "proxy.firm.example".to_owned(),
        port: 3128,
    };
    let given = args(&proxy);
    assert_eq!(
        after(&given, "--proxy"),
        Some(OsString::from("http://proxy.firm.example:3128"))
    );
    assert!(!given.iter().any(|arg| arg == "--noproxy"));
    // An IPv6 proxy gets its brackets.
    let six = Route::Http {
        host: "fd00::1".to_owned(),
        port: 8080,
    };
    assert_eq!(
        after(&args(&six), "--proxy"),
        Some(OsString::from("http://[fd00::1]:8080"))
    );
    // No proxy: curl is told so, so no variable of its own picks one.
    let direct = args(&Route::Direct);
    assert_eq!(after(&direct, "--noproxy"), Some(OsString::from("*")));
    assert!(!direct.iter().any(|arg| arg == "--proxy"));
}

#[test]
fn the_download_takes_the_route_of_network_upstream_proxy() {
    let dir = TempDir::new("cft-route");
    let tools = Fake::working();
    let mut job = job(dir.path(), Platform::MacArm64);
    let setting = url::Url::parse("http://proxy.firm.example:3128").unwrap();
    job.upstream = Some(Upstream::Fixed(setting));
    let (result, _) = run_with(&job, &tools, &Cancel::new());
    result.unwrap();
    assert_eq!(
        tools.routes(),
        [Route::Http {
            host: "proxy.firm.example".to_owned(),
            port: 3128,
        }]
    );
}

#[test]
fn a_refused_proxy_setting_fails_the_download_before_any_fetch() {
    let dir = TempDir::new("cft-route-refused");
    let tools = Fake::working();
    let mut job = job(dir.path(), Platform::MacArm64);
    job.upstream = None;
    let (result, _) = run_with(&job, &tools, &Cancel::new());
    assert_eq!(result.unwrap_err().code, BrowserInstallFailure::Network);
    assert_eq!(tools.calls(), Vec::<&str>::new());
    assert_eq!(super::browsers(dir.path()), Vec::<String>::new());
}
