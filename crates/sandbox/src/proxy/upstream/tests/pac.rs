//! The PAC settings: on Windows, which errors mean no proxy here, the order the lookups run in (a
//! PAC URL before auto-detect, so a slow WPAD cannot use up the lookup's bound), and each error's
//! [`FailKind`]; on macOS, each PAC run failure's.

use std::cell::RefCell;
use std::sync::atomic::AtomicUsize;
use std::sync::mpsc;
use std::time::Duration;

use super::super::parse::{
    CfPacFailure, PacAnswer, PacFrom, cf_pac_kind, pac_answer, pac_error, pac_kind,
};
use super::super::{FailKind, Route, UpstreamAnswer, bounded, settle};
use super::http;

/// A failed PAC lookup with nothing else set: from auto-detect, "no proxy here" (12180 no WPAD
/// file, 12167 a `wpad` name with no script, 12178 the service off) is `DIRECT`, and a bad script
/// (12166), a timeout (12002) and any code not expected are failed; from a PAC URL the firm named,
/// every error is failed, 12167 included (stage C's review). Each failure carries
/// [`pac_kind`]'s kind.
#[test]
fn a_pac_error_is_direct_only_for_no_proxy_here_from_auto_detect() {
    for error in [12_180, 12_167, 12_178] {
        let answer = settle(443, pac_error(PacFrom::AutoDetect, error));
        assert_eq!(answer, UpstreamAnswer::Route(Route::Direct), "{error}");
    }
    for error in [12_166, 12_002, 0, 1, 12_179, 12_181, u32::MAX] {
        let answer = settle(443, pac_error(PacFrom::AutoDetect, error));
        let kind = pac_kind(PacFrom::AutoDetect, error);
        assert_eq!(answer, UpstreamAnswer::Failed(kind), "auto-detect {error}");
    }
    for error in [12_167, 12_178, 12_180, 12_166, 12_002] {
        let answer = settle(443, pac_error(PacFrom::Url, error));
        let kind = pac_kind(PacFrom::Url, error);
        assert_eq!(answer, UpstreamAnswer::Failed(kind), "PAC URL {error}");
    }
}

/// A PAC URL that cannot be fetched (12167) or whose host is silent (12002 timeout, 12007 no
/// such name, 12029 no connection) is `PacUnreachable`; WPAD's never is. A script that will not
/// run (12166) is `BadScript` from either. The auto-proxy service's 12178, WPAD's 12180 and any
/// other code are `Other`.
#[test]
fn each_winhttp_pac_error_has_its_kind() {
    use FailKind::{BadScript, Other, PacUnreachable};
    let cases = [
        (12_167, PacUnreachable, Other),
        (12_002, PacUnreachable, Other),
        (12_007, PacUnreachable, Other),
        (12_029, PacUnreachable, Other),
        (12_166, BadScript, BadScript),
        (12_178, Other, Other),
        (12_180, Other, Other),
        (0, Other, Other),
        (u32::MAX, Other, Other),
    ];
    for (error, url, auto_detect) in cases {
        assert_eq!(pac_kind(PacFrom::Url, error), url, "PAC URL {error}");
        let kind = pac_kind(PacFrom::AutoDetect, error);
        assert_eq!(kind, auto_detect, "auto-detect {error}");
    }
}

/// macOS: a script that fails to evaluate (`kCFErrorDomainCFNetwork` 308) is `BadScript`, from a
/// PAC URL or a script held inline. A PAC URL's fetch failure (`NSURLErrorDomain`, or a negative
/// URL-loading code in `kCFErrorDomainCFNetwork`) or its run timing out is `PacUnreachable`; an
/// inline script never is. The rest are `Other`.
#[test]
fn each_macos_pac_failure_has_its_kind() {
    use FailKind::{BadScript, Other, PacUnreachable};
    let cf = "kCFErrorDomainCFNetwork";
    let url = "NSURLErrorDomain";
    let cases = [
        (CfPacFailure::Error(cf, 308), BadScript, BadScript),
        (CfPacFailure::Error(url, -1004), PacUnreachable, Other),
        (CfPacFailure::Error(url, -1003), PacUnreachable, Other),
        (CfPacFailure::Error(url, -1001), PacUnreachable, Other),
        (CfPacFailure::Error(cf, -1001), PacUnreachable, Other),
        (CfPacFailure::TimedOut, PacUnreachable, Other),
        (CfPacFailure::Error(cf, 310), Other, Other),
        (CfPacFailure::Error(cf, 0), Other, Other),
        (CfPacFailure::Error("kCFErrorDomainPOSIX", -1), Other, Other),
    ];
    for (failure, from_url, from_script) in cases {
        assert_eq!(cf_pac_kind(true, failure), from_url, "PAC URL {failure:?}");
        assert_eq!(
            cf_pac_kind(false, failure),
            from_script,
            "script {failure:?}"
        );
    }
}

/// A lookup per source: the PAC URL's and auto-detect's answers.
fn asks(
    url: Result<Route, u32>,
    auto_detect: Result<Route, u32>,
) -> impl FnMut(PacFrom) -> Result<Route, u32> {
    move |from: PacFrom| match from {
        PacFrom::Url => url.clone(),
        PacFrom::AutoDetect => auto_detect.clone(),
    }
}

/// Both set (a firm's GPO): the PAC URL is asked first and its route is the answer, auto-detect
/// never asked; a slow WPAD cannot use up the lookup's bound, so within it the PAC URL's route
/// comes back (with auto-detect first, the bound would end the lookup as failed).
#[test]
fn with_both_set_the_pac_url_answers_before_a_slow_auto_detect() {
    static THREADS: AtomicUsize = AtomicUsize::new(0);
    let asked = RefCell::new(Vec::new());
    let answer = pac_answer(true, true, |from| {
        asked.borrow_mut().push(from);
        match from {
            PacFrom::Url => Ok(http("pac-proxy", 1)),
            PacFrom::AutoDetect => Ok(http("wpad-proxy", 2)),
        }
    });
    assert_eq!(answer, PacAnswer::Final(Ok(http("pac-proxy", 1))));
    assert!(
        matches!(asked.borrow().as_slice(), [PacFrom::Url]),
        "auto-detect never asked"
    );
    let bounded_answer = bounded(Duration::from_millis(500), &THREADS, 1, || {
        let (_held, slow) = mpsc::channel::<()>();
        let answer = pac_answer(true, true, |from| match from {
            PacFrom::Url => Ok(http("pac-proxy", 1)),
            PacFrom::AutoDetect => {
                // A WPAD lookup that takes 3 s, past the bound.
                let _waited = slow.recv_timeout(Duration::from_secs(3));
                Ok(http("wpad-proxy", 2))
            }
        });
        match answer {
            PacAnswer::Final(answer) | PacAnswer::Fallback(answer) => answer.map_err(|e| e.why),
            PacAnswer::Unset => Ok(Route::Direct),
        }
    });
    assert_eq!(bounded_answer, Ok(http("pac-proxy", 1)));
}

/// A PAC URL that fails: auto-detect's route when it finds one, else the PAC URL's failure
/// (WPAD's "no proxy here" does not make it direct). Auto-detect alone: its route, or its error
/// as the fallback a static proxy answers instead of. Neither set: unset.
#[test]
fn a_failed_pac_url_falls_to_wpad_route_else_fails_and_auto_detect_alone_is_a_fallback() {
    let ask = asks(Err(12_167), Ok(http("wpad-proxy", 2)));
    assert_eq!(
        pac_answer(true, true, ask),
        PacAnswer::Final(Ok(http("wpad-proxy", 2)))
    );
    let ask = asks(Err(12_167), Err(12_180));
    let PacAnswer::Final(failed) = pac_answer(true, true, ask) else {
        panic!("a failed PAC URL is the answer");
    };
    let unreachable = UpstreamAnswer::Failed(FailKind::PacUnreachable);
    assert_eq!(settle(443, failed), unreachable, "the PAC URL's kind");
    let ask = asks(Err(12_002), Ok(http("unused", 3)));
    let PacAnswer::Final(failed) = pac_answer(false, true, ask) else {
        panic!("a failed PAC URL is the answer");
    };
    assert_eq!(settle(443, failed), unreachable);
    let ask = asks(Err(12_166), Err(12_166));
    let PacAnswer::Final(failed) = pac_answer(true, true, ask) else {
        panic!("a failed PAC URL is the answer");
    };
    let bad_script = UpstreamAnswer::Failed(FailKind::BadScript);
    assert_eq!(settle(443, failed), bad_script);
    let ask = asks(Ok(http("unused", 3)), Ok(http("wpad-proxy", 2)));
    assert_eq!(
        pac_answer(true, false, ask),
        PacAnswer::Final(Ok(http("wpad-proxy", 2)))
    );
    let ask = asks(Ok(http("unused", 3)), Err(12_180));
    assert_eq!(
        pac_answer(true, false, ask),
        PacAnswer::Fallback(Ok(Route::Direct))
    );
    let ask = asks(Ok(http("unused", 3)), Err(12_166));
    let PacAnswer::Fallback(failed) = pac_answer(true, false, ask) else {
        panic!("auto-detect alone is a fallback");
    };
    assert_eq!(settle(443, failed), bad_script);
    let ask = asks(Ok(http("unused", 3)), Ok(http("unused", 4)));
    assert_eq!(pac_answer(false, false, ask), PacAnswer::Unset);
}

/// macOS, run for real: a PAC URL on a closed loopback port is `PacUnreachable`, and a script
/// that throws is `BadScript`.
#[cfg(target_os = "macos")]
#[test]
fn macos_pac_failures_have_their_kinds() {
    use super::super::macos;
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let pac = format!("http://127.0.0.1:{port}/proxy.pac");
    let failed = macos::url_route(&pac, "https://via-proxy.test/").unwrap_err();
    assert_eq!(failed.kind, FailKind::PacUnreachable, "{failed:?}");
    let script = r#"function FindProxyForURL(url, host) { throw new Error("broken"); }"#;
    let failed = macos::script_route(script, "https://via-proxy.test/").unwrap_err();
    assert_eq!(failed.kind, FailKind::BadScript, "{failed:?}");
}
