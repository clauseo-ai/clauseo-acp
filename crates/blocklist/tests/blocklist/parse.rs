//! The list's text: good, malformed, unsorted, duplicate, miscounted and too big.

use std::io::Write;

use clauseo_blocklist::{Error, MAX_ENTRIES, MAX_TEXT, Tag, parse_gz, parse_plain};

use crate::fixtures::{header, plain, with_comments};

fn parse(entries: &[&str], said: u32) -> Result<clauseo_blocklist::BlockList, Error> {
    parse_plain(with_comments(entries).as_bytes(), header(said))
}

fn gzip(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    encoder.write_all(bytes).unwrap();
    encoder.finish().unwrap()
}

#[test]
fn a_good_list_parses_with_its_tags_and_time() {
    let list = plain(&[
        "1.2.3.4 m",
        "[2001:db8::1] m",
        "a-b.example p",
        "xn--80ak6aa92e.com p",
    ]);
    assert_eq!(list.len(), 4);
    assert_eq!(list.generated().unwrap().as_str(), "2026-11-07T06:00:00Z");
    assert_eq!(list.lookup("a-b.example").unwrap().tag, Tag::Phishing);
    assert_eq!(list.lookup("1.2.3.4").unwrap().tag, Tag::Malware);
    assert!(!list.is_unloaded());
}

#[test]
fn the_same_list_gzipped_parses_the_same() {
    let text = with_comments(&["evil.example p", "phish.test p"]);
    let list = parse_gz(&gzip(text.as_bytes()), header(2)).unwrap();
    assert_eq!(list, plain(&["evil.example p", "phish.test p"]));
}

#[test]
fn the_sources_header_is_opaque() {
    let text = "# clauseo-blocklist v1\n# sources: anything at all = ; ; =\nphish.test p\n";
    assert_eq!(parse_plain(text.as_bytes(), header(1)).unwrap().len(), 1);
}

#[test]
fn a_list_of_no_entries_is_loaded_but_empty() {
    let list = parse(&[], 0).unwrap();
    assert!(list.is_empty());
    assert!(!list.is_unloaded());
}

#[test]
fn malformed_lines_are_refused_with_their_line_number() {
    let bad = [
        "Evil.example p",         // uppercase
        "evil.example. p",        // trailing dot
        "evil.example x",         // unknown tag
        "evil.example  p",        // two spaces
        "evil.example p extra",   // trailing words
        "evil.example",           // no tag
        "https://evil.example p", // scheme
        "evil.example:443 p",     // port
        "evil.example/path p",    // path
        "user@evil.example p",    // user info
        "com p",                  // a bare TLD
        "-evil.example p",        // a label starting with -
        "evil..example p",        // an empty label
        "1.2.3 p",                // an all-digit last label
        "[2001:db8::zz] m",       // a bad IPv6
        "2001:db8::1 m",          // IPv6 without brackets
        "evil.example p\r",       // CRLF
        "",                       // an empty line
        "\u{feff}evil.example p", // a BOM
        "évil.example p",         // not ASCII
    ];
    for line in bad {
        assert_eq!(
            parse(&[line], 1).map(|_| ()),
            Err(Error::Malformed { line: 4 }),
            "{line:?}"
        );
    }
}

#[test]
fn an_unsorted_list_is_refused() {
    assert_eq!(
        parse(&["phish.test p", "evil.example p"], 2).map(|_| ()),
        Err(Error::Unsorted { line: 5 })
    );
}

#[test]
fn a_duplicate_host_is_refused_whatever_its_tag() {
    assert_eq!(
        parse(&["evil.example m", "evil.example p"], 2).map(|_| ()),
        Err(Error::Duplicate { line: 5 })
    );
}

#[test]
fn an_entry_count_that_differs_from_the_comment_is_refused() {
    let entries = ["evil.example p", "phish.test p"];
    assert_eq!(
        parse(&entries, 3).map(|_| ()),
        Err(Error::EntriesMismatch { said: 3, found: 2 })
    );
    assert_eq!(
        parse(&entries, 1).map(|_| ()),
        Err(Error::EntriesMismatch { said: 1, found: 2 })
    );
}

#[test]
fn more_entries_than_the_bound_is_refused_before_parsing() {
    assert_eq!(parse(&[], MAX_ENTRIES + 1).map(|_| ()), Err(Error::TooBig));
}

#[test]
fn more_text_than_the_bound_is_refused_and_a_gzip_bomb_stops_there() {
    let mut text = with_comments(&["evil.example p"]).into_bytes();
    let comment = vec![b'#'; usize::try_from(MAX_TEXT).unwrap()];
    text.extend_from_slice(&comment);
    text.push(b'\n');
    assert_eq!(
        parse_plain(&text, header(1)).map(|_| ()),
        Err(Error::TooBig)
    );
    let bomb = gzip(&text);
    assert!(
        bomb.len() < 1024 * 1024,
        "the fixture should be small: {}",
        bomb.len()
    );
    assert_eq!(parse_gz(&bomb, header(1)).map(|_| ()), Err(Error::TooBig));
}

#[test]
fn bytes_that_are_not_gzip_are_refused() {
    assert_eq!(
        parse_gz(b"not gzip", header(0)).map(|_| ()),
        Err(Error::Gzip)
    );
}
