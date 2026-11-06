use std::io;

use super::{Failure, Token, decide};

/// The fake process's user.
const USER: &str = "S-1-5-21-1-2-3-1001";
/// Another account's SID.
const OTHER: &str = "S-1-5-21-1-2-3-1002";
/// A third account's SID.
const THIRD: &str = "S-1-5-21-1-2-3-1003";

/// One read's answer, made fresh for each call (`Failure` holds an `io::Error`, which is not
/// `Clone`).
#[derive(Clone, Copy)]
enum Answer {
    Yes,
    No,
    Denied,
    Broken,
}

impl Answer {
    fn read(self) -> Result<bool, Failure> {
        match self {
            Self::Yes => Ok(true),
            Self::No => Ok(false),
            Self::Denied => Err(Failure::Denied),
            Self::Broken => Err(Failure::Other(io::Error::other("broken"))),
        }
    }
}

/// A token with fixed answers. `user_is` compares with [`USER`] and, as
/// `ConvertStringSidToSidW` would, says [`Failure::NotASid`] for a string not starting `S-1-`.
#[derive(Clone, Copy)]
struct Fake {
    app_container: Answer,
    restricted: Answer,
    /// `below_medium`'s answer.
    low: Answer,
    /// `None`: `user_is` compares; else this answer.
    user: Option<Answer>,
}

/// An unjailed token.
const PLAIN: Fake = Fake {
    app_container: Answer::No,
    restricted: Answer::No,
    low: Answer::No,
    user: None,
};

impl Token for Fake {
    fn app_container(&self) -> Result<bool, Failure> {
        self.app_container.read()
    }

    fn restricted(&self) -> Result<bool, Failure> {
        self.restricted.read()
    }

    fn below_medium(&self) -> Result<bool, Failure> {
        self.low.read()
    }

    fn user_is(&self, sid: &str) -> Result<bool, Failure> {
        match self.user {
            Some(answer) => answer.read(),
            None if !sid.starts_with("S-1-") => Err(Failure::NotASid),
            None => Ok(sid == USER),
        }
    }
}

/// A pool record that names the one SID `sid`.
fn record(sid: &'static str) -> impl FnOnce() -> Result<Vec<String>, Failure> {
    move || Ok(vec![sid.to_owned()])
}

/// A pool record that names `sids`.
fn pool(sids: &'static [&'static str]) -> impl FnOnce() -> Result<Vec<String>, Failure> {
    move || Ok(sids.iter().map(|sid| (*sid).to_owned()).collect())
}

/// No pool record.
fn no_record() -> impl FnOnce() -> Result<Vec<String>, Failure> {
    || Ok(Vec::new())
}

/// `decide` with `Ok(token)`, as a short string: "jailed", "free" or "error: <message>".
fn verdict(token: Fake, record: impl FnOnce() -> Result<Vec<String>, Failure>) -> String {
    show(decide(Ok(token), record))
}

fn show(answer: io::Result<bool>) -> String {
    match answer {
        Ok(true) => "jailed".to_owned(),
        Ok(false) => "free".to_owned(),
        Err(error) => format!("error: {error}"),
    }
}

#[test]
fn no_signal_and_no_pool_record_is_not_sandboxed() {
    assert_eq!(verdict(PLAIN, no_record()), "free");
}

#[test]
fn a_pool_user_is_sandboxed_and_any_other_user_is_not() {
    assert_eq!(verdict(PLAIN, record(USER)), "jailed");
    assert_eq!(verdict(PLAIN, record(OTHER)), "free");
}

#[test]
fn the_user_is_compared_with_every_pool_sid() {
    assert_eq!(verdict(PLAIN, pool(&[OTHER, THIRD, USER])), "jailed");
    assert_eq!(verdict(PLAIN, pool(&[USER, OTHER])), "jailed");
    assert_eq!(verdict(PLAIN, pool(&[OTHER, THIRD])), "free");
    // A SID in the list that does not parse counts as sandboxed, wherever it is.
    assert_eq!(verdict(PLAIN, pool(&[OTHER, "BA"])), "jailed");
}

#[test]
fn a_malformed_pool_record_is_sandboxed() {
    // `windows::sandbox_pool_sids` maps a malformed `PoolSids` (an empty string inside, a
    // non-SID, a repeat) and one that is not UTF-16 to `NotASid`.
    assert_eq!(verdict(PLAIN, || Err(Failure::NotASid)), "jailed");
}

#[test]
fn a_record_that_is_denied_the_wrong_type_or_not_a_sid_is_sandboxed() {
    assert_eq!(verdict(PLAIN, || Err(Failure::Denied)), "jailed");
    assert_eq!(verdict(PLAIN, || Err(Failure::WrongType)), "jailed");
    assert_eq!(verdict(PLAIN, record("not a sid")), "jailed");
    assert_eq!(verdict(PLAIN, record("")), "jailed");
}

#[test]
fn any_other_record_failure_fails_open_with_its_error() {
    let broken = || Err(Failure::Other(io::Error::other("registry broke")));
    assert_eq!(verdict(PLAIN, broken), "error: registry broke");
}

#[test]
fn an_app_container_or_restricting_sids_are_sandboxed_without_the_record() {
    let read = std::cell::Cell::new(false);
    let watched = || {
        read.set(true);
        Ok(Vec::new())
    };
    let container = Fake {
        app_container: Answer::Yes,
        ..PLAIN
    };
    assert_eq!(verdict(container, watched), "jailed");
    let restricted = Fake {
        restricted: Answer::Yes,
        ..PLAIN
    };
    assert_eq!(verdict(restricted, no_record()), "jailed");
    assert!(!read.get(), "a positive token signal needs no record");
}

#[test]
fn a_token_that_cannot_be_opened_for_access_denied_is_sandboxed() {
    assert_eq!(
        show(decide::<Fake>(Err(Failure::Denied), no_record())),
        "jailed"
    );
    assert_eq!(
        show(decide::<Fake>(
            Err(Failure::Other(io::Error::other("no token"))),
            no_record()
        )),
        "error: no token"
    );
}

#[test]
fn each_token_read_denied_is_sandboxed() {
    let denied_container = Fake {
        app_container: Answer::Denied,
        ..PLAIN
    };
    let denied_restricted = Fake {
        restricted: Answer::Denied,
        ..PLAIN
    };
    let denied_user = Fake {
        user: Some(Answer::Denied),
        ..PLAIN
    };
    assert_eq!(verdict(denied_container, no_record()), "jailed");
    assert_eq!(verdict(denied_restricted, no_record()), "jailed");
    assert_eq!(verdict(denied_user, record(OTHER)), "jailed");
}

#[test]
fn a_broken_read_fails_open_only_when_no_other_signal_answers() {
    let broken_container = Fake {
        app_container: Answer::Broken,
        ..PLAIN
    };
    assert_eq!(verdict(broken_container, no_record()), "error: broken");
    assert_eq!(verdict(broken_container, record(USER)), "jailed");
    let and_restricted = Fake {
        restricted: Answer::Yes,
        ..broken_container
    };
    assert_eq!(verdict(and_restricted, no_record()), "jailed");
    let broken_user = Fake {
        user: Some(Answer::Broken),
        ..PLAIN
    };
    assert_eq!(verdict(broken_user, record(USER)), "error: broken");
}

#[test]
fn low_integrity_is_sandboxed_whatever_the_record_says() {
    let low = Fake {
        low: Answer::Yes,
        ..PLAIN
    };
    assert_eq!(verdict(low, no_record()), "jailed");
    assert_eq!(verdict(low, record(OTHER)), "jailed");
    // Medium (or above) with no pool: not sandboxed.
    assert_eq!(verdict(PLAIN, no_record()), "free");
}

#[test]
fn an_integrity_level_that_cannot_be_read_is_sandboxed() {
    for unreadable in [Answer::Denied, Answer::Broken] {
        let token = Fake {
            low: unreadable,
            ..PLAIN
        };
        assert_eq!(verdict(token, no_record()), "jailed");
    }
}
