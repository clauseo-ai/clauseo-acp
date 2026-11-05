//! Which typed text is codex-acp's `/logout`.

use super::starts_logout;

#[test]
fn slash_logout_alone_or_with_more_text_is_the_command() {
    for text in [
        "/logout",
        "  /logout",
        "/LogOut",
        "/logout now",
        "\n/logout\tplease",
    ] {
        assert!(starts_logout(text), "{text:?}");
    }
}

#[test]
fn other_text_is_not_the_command() {
    for text in [
        "logout",
        "/logouts",
        "/log out",
        "please /logout",
        "/",
        "",
        "//logout",
    ] {
        assert!(!starts_logout(text), "{text:?}");
    }
}
