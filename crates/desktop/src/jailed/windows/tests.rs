//! The real Win32 calls, on the Windows CI job (outside any sandbox).

use windows::Win32::Foundation::{HLOCAL, LocalFree};
use windows::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows::Win32::Security::{TOKEN_USER, TokenUser};
use windows::core::PWSTR;

use super::super::signals::{Failure, Token as _};
use super::{LocalSid, Token, sandbox_pool_sids};

/// This process's user SID, spelled as `ConvertSidToStringSidW` spells it.
fn own_sid(token: &Token) -> String {
    let buffer = token.variable::<TOKEN_USER>(TokenUser).unwrap();
    // SAFETY: as in `user_is`: a TOKEN_USER at the start of an aligned buffer that outlives it.
    let user = unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() };
    let mut text = PWSTR::null();
    // SAFETY: the SID is valid; `text` is a live out-pointer, freed with LocalFree below.
    unsafe { ConvertSidToStringSidW(user.User.Sid, &raw mut text) }.unwrap();
    // SAFETY: ConvertSidToStringSidW wrote a NUL-terminated string.
    let sid = unsafe { text.to_string() }.unwrap();
    // SAFETY: allocated by ConvertSidToStringSidW with LocalAlloc, freed once.
    unsafe { LocalFree(Some(HLOCAL(text.0.cast()))) };
    sid
}

#[test]
fn the_token_user_compares_as_a_sid() {
    let token = Token::open()
        .map_err(|failure| format!("{failure:?}"))
        .unwrap();
    let sid = own_sid(&token);
    assert!(matches!(token.user_is(&sid), Ok(true)), "{sid}");
    // Everyone (S-1-1-0) is never a process's user.
    assert!(matches!(token.user_is("S-1-1-0"), Ok(false)));
    for garbage in ["", "not a sid", "S-1-x"] {
        assert!(
            matches!(token.user_is(garbage), Err(Failure::NotASid)),
            "{garbage:?}"
        );
    }
}

#[test]
fn an_unsandboxed_token_has_no_signal() {
    let token = Token::open()
        .map_err(|failure| format!("{failure:?}"))
        .unwrap();
    assert!(matches!(token.app_container(), Ok(false)));
    assert!(matches!(token.restricted(), Ok(false)));
    // The CI runner runs at Medium or High.
    assert!(matches!(token.below_medium(), Ok(false)));
}

#[test]
fn the_pool_record_reads() {
    // The registry read itself is tested in clauseo-sandbox (`windows/tests.rs`). The real
    // record: a CI runner has none, so no pool; a developer's machine may.
    let record = sandbox_pool_sids();
    assert!(record.is_ok(), "{record:?}");
}

#[test]
fn only_the_s_form_of_a_sid_parses() {
    // Aliases ConvertStringSidToSidW accepts name a group, not one account's SID.
    for alias in ["BA", "WD"] {
        assert!(
            matches!(LocalSid::parse(alias), Err(Failure::NotASid)),
            "{alias:?}"
        );
    }
    assert!(LocalSid::parse("S-1-5-32-544").is_ok());
}
