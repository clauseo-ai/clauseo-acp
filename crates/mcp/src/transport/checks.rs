//! The command-line checks of the Windows pipe options, kept free of Windows calls so they are
//! tested on every OS: the pipe name the host picks (`--listen-pipe`, `--pipe`) and the shape of a
//! SID string (`--client-sid`, `--server-sid`, `--client-package-sid`). Windows itself has the last
//! word on a SID (`windows/sid.rs` converts it); this only keeps anything but `S-1-...` digits out.

/// The longest pipe name taken. Windows allows 256 characters for the whole `\\.\pipe\` path.
pub(crate) const PIPE_NAME_MAX: usize = 200;

/// The most sub-authorities a SID holds.
const SUB_AUTHORITIES_MAX: usize = 15;

/// Whether `name` may name the pipe: 1 to [`PIPE_NAME_MAX`] ASCII letters, digits and dashes,
/// so it can hold no path separator, no `..` and nothing a log could be confused by.
pub(crate) fn pipe_name_ok(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= PIPE_NAME_MAX
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

/// Whether `text` has a SID string's shape: `S-1-`, a decimal authority, then 1 to 15 decimal
/// sub-authorities, all dash-separated. No alias (`WD`, `BA`) and no hex authority.
pub(crate) fn sid_text_ok(text: &str) -> bool {
    let Some(rest) = text.strip_prefix("S-1-") else {
        return false;
    };
    let parts: Vec<&str> = rest.split('-').collect();
    (2..=SUB_AUTHORITIES_MAX + 1).contains(&parts.len())
        && parts.iter().all(|part| {
            !part.is_empty() && part.len() <= 10 && part.bytes().all(|byte| byte.is_ascii_digit())
        })
}

/// Whether `text` has an `AppContainer` package SID's shape: [`sid_text_ok`], `S-1-15-2-` then
/// the seven sub-authorities a package name derives, so never ALL APPLICATION PACKAGES
/// (`S-1-15-2-1`) or ALL RESTRICTED APPLICATION PACKAGES (`S-1-15-2-2`), which would let any
/// `LowBox` process in, Chrome's included. The host derives it from the agent's package name (stage
/// C's `crates/lowbox`); the guard only checks and compares it.
pub(crate) fn package_sid_text_ok(text: &str) -> bool {
    sid_text_ok(text)
        && text
            .strip_prefix("S-1-15-2-")
            .is_some_and(|rids| rids.split('-').count() == 7)
}

#[cfg(test)]
mod tests {
    use super::{PIPE_NAME_MAX, package_sid_text_ok, pipe_name_ok, sid_text_ok};

    #[test]
    fn a_pipe_name_is_letters_digits_and_dashes() {
        assert!(pipe_name_ok("clauseo-ab-3f9c0e21"));
        assert!(pipe_name_ok("A"));
        assert!(pipe_name_ok(&"a".repeat(PIPE_NAME_MAX)));
        for bad in [
            "",
            "a b",
            "a\\b",
            "a/b",
            "..",
            "a.b",
            "a_b",
            "a:b",
            "pipe\0",
            "\u{e9}t\u{e9}",
            &"a".repeat(PIPE_NAME_MAX + 1),
        ] {
            assert!(!pipe_name_ok(bad), "{bad:?}");
        }
    }

    #[test]
    fn a_sid_string_has_the_s_1_shape() {
        for good in [
            "S-1-5-18",
            "S-1-16-4096",
            "S-1-5-21-3623811015-3361044348-30300820-1013",
            "S-1-1-0",
        ] {
            assert!(sid_text_ok(good), "{good}");
        }
        for bad in [
            "",
            "WD",
            "BA",
            "S-1-5",
            "S-2-5-18",
            "s-1-5-18",
            "S-1-5-18-",
            "S-1--5-18",
            "S-1-0x5-18",
            "S-1-5-18)(A;;GA;;;WD",
            " S-1-5-18",
            "S-1-5-99999999999",
            "S-1-5-1-2-3-4-5-6-7-8-9-10-11-12-13-14-15-16",
        ] {
            assert!(!sid_text_ok(bad), "{bad:?}");
        }
    }

    #[test]
    fn a_package_sid_is_an_appcontainer_sid() {
        assert!(package_sid_text_ok(
            "S-1-15-2-2434737943-167758768-3180539153-984336765-1107280622-3591121930-2677285773"
        ));
        for bad in [
            "S-1-5-18",
            "S-1-15-3-1024-1-2-3-4-5-6-7-8",
            "S-1-15-2-1",
            "S-1-15-2-2",
            "S-1-15-2-1-2-3-4-5-6",
            "S-1-15-2-1-2-3-4-5-6-7-8",
            "S-1-15-2",
            "S-1-15-2-",
            "S-1-15-20-1",
            "S-1-15-2-1)(A;;GA;;;WD",
            "",
        ] {
            assert!(!package_sid_text_ok(bad), "{bad:?}");
        }
    }
}
