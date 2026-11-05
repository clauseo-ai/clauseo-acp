//! The real Win32 calls, on the Windows CI job (outside any sandbox).

use windows::Win32::Foundation::{HLOCAL, LocalFree};
use windows::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows::Win32::Security::{TOKEN_USER, TokenUser};
use windows::core::{PWSTR, w};

use super::super::signals::{Failure, Token as _};
use super::{LocalSid, Token, read_string, sandbox_user_sid};

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
}

#[test]
fn the_registry_read_tells_missing_wrong_type_and_a_string_apart() {
    let missing = read_string(w!("SOFTWARE\\Clauseo\\NoSuchKeyForTests"), w!("UserSid"));
    assert!(matches!(missing, Ok(None)), "{missing:?}");
    // Values every Windows has: ProductName is a REG_SZ, InstallDate a REG_DWORD.
    let version = w!("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion");
    let name = read_string(version, w!("ProductName"));
    assert!(
        matches!(&name, Ok(Some(text)) if text.starts_with("Windows")),
        "{name:?}"
    );
    let missing_value = read_string(version, w!("NoSuchValueForTests"));
    assert!(matches!(missing_value, Ok(None)), "{missing_value:?}");
    let dword = read_string(version, w!("InstallDate"));
    assert!(matches!(dword, Err(Failure::WrongType)), "{dword:?}");
    // REG_EXPAND_SZ is refused unexpanded: without RRF_NOEXPAND it would come back expanded, e.g.
    // `C:\Program Files`, and the environment (which the agent controls) would choose the text.
    let expand = read_string(
        w!("SOFTWARE\\Microsoft\\Windows\\CurrentVersion"),
        w!("ProgramFilesPath"),
    );
    assert!(matches!(expand, Err(Failure::WrongType)), "{expand:?}");
    // The real record: a CI runner has none, a developer's machine may.
    let record = sandbox_user_sid();
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
