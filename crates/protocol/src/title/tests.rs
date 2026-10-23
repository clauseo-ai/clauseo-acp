//! The order of the title checks: length first, then a visible character, then the characters
//! a title may not hold.

use crate::{Error, MAX_TITLE_CHARS, check_display_title};

#[test]
fn a_long_title_of_spaces_is_too_large_not_invisible() {
    let long = " ".repeat(1 << 20);
    assert!(
        matches!(
            check_display_title(&long),
            Err(Error::TooLarge {
                kind: "Title",
                size,
                max: MAX_TITLE_CHARS,
                ..
            }) if size == 1 << 20
        ),
        "a 1 MiB title of spaces is TooLarge"
    );
    // At the limit the length passes, and the title has no visible character.
    let at_limit = " ".repeat(MAX_TITLE_CHARS);
    assert!(
        matches!(
            check_display_title(&at_limit),
            Err(Error::Format { kind: "Title", .. })
        ),
        "200 spaces hold no visible character"
    );
    assert!(check_display_title(&format!("{}a", " ".repeat(MAX_TITLE_CHARS - 1))).is_ok());
}
