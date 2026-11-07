//! The Windows PAC settings: which errors mean no proxy here, and the order the lookups run in
//! (a PAC URL before auto-detect, so a slow WPAD cannot use up the lookup's bound).

use std::cell::RefCell;
use std::sync::atomic::AtomicUsize;
use std::sync::mpsc;
use std::time::Duration;

use super::super::parse::{PacAnswer, PacFrom, pac_answer, pac_error};
use super::super::{Route, UpstreamAnswer, bounded, settle};
use super::http;

/// A failed PAC lookup with nothing else set: from auto-detect, "no proxy here" (12180 no WPAD
/// file, 12167 a `wpad` name with no script, 12178 the service off) is `DIRECT`, and a bad script
/// (12166), a timeout (12002) and any code not expected are failed; from a PAC URL the firm named,
/// every error is failed, 12167 included (stage C's review).
#[test]
fn a_pac_error_is_direct_only_for_no_proxy_here_from_auto_detect() {
    for error in [12_180, 12_167, 12_178] {
        let answer = settle(443, pac_error(PacFrom::AutoDetect, error));
        assert_eq!(answer, UpstreamAnswer::Route(Route::Direct), "{error}");
    }
    for error in [12_166, 12_002, 0, 1, 12_179, 12_181, u32::MAX] {
        let answer = settle(443, pac_error(PacFrom::AutoDetect, error));
        assert_eq!(answer, UpstreamAnswer::Failed, "auto-detect {error}");
    }
    for error in [12_167, 12_178, 12_180, 12_166, 12_002] {
        let answer = settle(443, pac_error(PacFrom::Url, error));
        assert_eq!(answer, UpstreamAnswer::Failed, "PAC URL {error}");
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
            PacAnswer::Final(answer) | PacAnswer::Fallback(answer) => answer,
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
    assert_eq!(settle(443, failed), UpstreamAnswer::Failed);
    let ask = asks(Err(12_002), Ok(http("unused", 3)));
    let PacAnswer::Final(failed) = pac_answer(false, true, ask) else {
        panic!("a failed PAC URL is the answer");
    };
    assert_eq!(settle(443, failed), UpstreamAnswer::Failed);
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
    assert!(matches!(
        pac_answer(true, false, ask),
        PacAnswer::Fallback(Err(_))
    ));
    let ask = asks(Ok(http("unused", 3)), Ok(http("unused", 4)));
    assert_eq!(pac_answer(false, false, ask), PacAnswer::Unset);
}
