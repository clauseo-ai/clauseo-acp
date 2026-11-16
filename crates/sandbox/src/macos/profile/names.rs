//! The path-free patterns of the matter's name rules, each paired in the profile with
//! `(subpath (param "MATTER"))`. Seatbelt's regex has no case flag, so each letter is a class of
//! both cases, and it matches UTF-8 bytes, so a code point is its literal byte sequence.

/// [`INSTRUCTIONS_AT_ANY_DEPTH`](crate::INSTRUCTIONS_AT_ANY_DEPTH) as a pattern on the last
/// name.
pub(super) const INSTRUCTIONS_REGEX: &str = "/[cC][lL][aA][uU][dD][eE]\\.[mM][dD]$";

/// An optional run of the code points case-insensitive HFS+ ignores in names
/// ([`hfs_ignorable`](crate::hfs_ignorable)), each a literal: a bracket range would range over
/// bytes. Written `((...)+)?` because Seatbelt reads `(...)*` as one or more (checked on this
/// Mac: `/\.g(x|y)*it$` did not match `.git`).
macro_rules! ignorables {
    () => {
        "((\u{200C}|\u{200D}|\u{200E}|\u{200F}|\u{202A}|\u{202B}|\u{202C}|\u{202D}|\u{202E}\
         |\u{206A}|\u{206B}|\u{206C}|\u{206D}|\u{206E}|\u{206F}|\u{FEFF})+)?"
    };
}

/// Any path name `.git`, in any letter case, as the last name or a folder above it, with HFS+'s
/// ignorables anywhere in it. A `.git` the agent made would be the lawyer's and run its hooks
/// under the lawyer's own git.
pub(super) const GIT_NAME_REGEX: &str = concat!(
    "/",
    ignorables!(),
    "\\.",
    ignorables!(),
    "[gG]",
    ignorables!(),
    "[iI]",
    ignorables!(),
    "[tT]",
    ignorables!(),
    "(/|$)"
);

/// A last name `HEAD`, in any letter case, with HFS+'s ignorables anywhere in it: git takes a
/// folder holding `HEAD`, `objects/` and `refs/` for a bare repository, with no `.git` name, and
/// it needs `HEAD` in the git dir.
pub(super) const HEAD_REGEX: &str = concat!(
    "/",
    ignorables!(),
    "[hH]",
    ignorables!(),
    "[eE]",
    ignorables!(),
    "[aA]",
    ignorables!(),
    "[dD]",
    ignorables!(),
    "$"
);
