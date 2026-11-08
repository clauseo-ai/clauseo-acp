//! Which typed text is codex-acp's `/logout`, or `/login`.

use super::{LOGIN, LOGOUT, starts_command};

fn starts_logout(text: &str) -> bool {
    starts_command(text, LOGOUT)
}

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

#[test]
fn slash_login_is_its_own_command() {
    assert!(starts_command(" /Login please", LOGIN));
    assert!(!starts_command("/logins", LOGIN));
    assert!(!starts_command("/login", LOGOUT));
}
