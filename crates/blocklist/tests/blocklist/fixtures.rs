//! The signed fixtures and the test keys.

use clauseo_blocklist::{BlockList, Generated, Header};

/// A signed list: the `.gz` bytes and its `.minisig`.
pub(crate) struct Signed {
    pub(crate) gz: &'static [u8],
    pub(crate) minisig: &'static [u8],
}

macro_rules! signed {
    ($stem:literal) => {
        Signed {
            gz: include_bytes!(concat!("../data/", $stem, ".txt.gz")),
            minisig: include_bytes!(concat!("../data/", $stem, ".txt.gz.minisig")),
        }
    };
}

/// 6 entries, made 2026-11-07T06:00:00Z, signed by key A.
pub(crate) const LIST: Signed = signed!("list");
/// 7 entries (`newer.test` added), made 2026-11-07T12:00:00Z, key A.
pub(crate) const NEWER: Signed = signed!("list-newer");
/// 3 entries, made 2026-11-06T00:00:00Z, key A.
pub(crate) const OLDER: Signed = signed!("list-older");
/// `LIST`'s content signed by key B.
pub(crate) const BY_B: Signed = signed!("list-by-b");
/// `LIST`'s content signed by key C, which no test set holds.
pub(crate) const BY_C: Signed = signed!("list-by-c");
/// `LIST`'s content with a trusted comment of 7 entries, key A.
pub(crate) const MISCOUNTED: Signed = signed!("list-miscounted");
/// `LIST` signed by key A the legacy way (`Ed`, not prehashed).
pub(crate) const LEGACY: Signed = signed!("list-legacy");

/// The key line (second line) of a minisign public key file.
fn key_line(file: &'static str) -> &'static str {
    file.lines().nth(1).unwrap()
}

pub(crate) fn key_a() -> &'static str {
    key_line(include_str!("../data/key-a.pub"))
}

pub(crate) fn key_b() -> &'static str {
    key_line(include_str!("../data/key-b.pub"))
}

pub(crate) fn key_c() -> &'static str {
    key_line(include_str!("../data/key-c.pub"))
}

pub(crate) fn generated(text: &str) -> Generated {
    Generated::parse(text).unwrap()
}

/// A header for parse tests that skip the signature.
pub(crate) fn header(entries: u32) -> Header {
    Header {
        generated: generated("2026-11-07T06:00:00Z"),
        entries,
    }
}

/// Parses `lines` (each ending in LF) under the v1 comments, with the right entry count.
pub(crate) fn plain(entries: &[&str]) -> BlockList {
    let text = with_comments(entries);
    clauseo_blocklist::parse_plain(
        text.as_bytes(),
        header(u32::try_from(entries.len()).unwrap()),
    )
    .unwrap()
}

pub(crate) fn with_comments(entries: &[&str]) -> String {
    let mut text = String::from(
        "# clauseo-blocklist v1\n# generated: 2026-11-07T06:00:00Z\n\
         # sources: urlhaus=none; phishing-database=abc; feodo=x\n",
    );
    for entry in entries {
        text.push_str(entry);
        text.push('\n');
    }
    text
}
