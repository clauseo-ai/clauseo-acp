//! The hello's shape check and the capped line reader.

use std::io::Cursor;

use super::*;

const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn hello(fields: &str) -> Vec<u8> {
    format!("{{{fields}}}").into_bytes()
}

#[test]
fn a_hello_of_exactly_the_three_keys_parses() {
    let line = hello(&format!(
        r#""clauseo_relay":1,"token":"{TOKEN}","connector":"case-law""#
    ));
    let parsed = parse(&line).ok().unwrap();
    assert_eq!((parsed.token, parsed.connector), (TOKEN, "case-law"));
    // Key order is free.
    let line = hello(&format!(
        r#""connector":"a","token":"{TOKEN}","clauseo_relay":1"#
    ));
    assert!(parse(&line).is_ok());
}

#[test]
fn only_a_numeric_other_version_is_unsupported() {
    for version in ["2", "1.5", "0", "-1"] {
        let line = hello(&format!(
            r#""clauseo_relay":{version},"token":"{TOKEN}","connector":"c""#
        ));
        assert_eq!(
            parse(&line).err(),
            Some(Refusal::Answer(texts::UNSUPPORTED_VERSION)),
            "{version}"
        );
    }
    for version in [r#""1""#, "true", "null", "[1]"] {
        let line = hello(&format!(
            r#""clauseo_relay":{version},"token":"{TOKEN}","connector":"c""#
        ));
        assert_eq!(
            parse(&line).err(),
            Some(Refusal::Silent("bad_hello")),
            "{version}"
        );
    }
}

#[test]
fn tokens_and_names_outside_the_patterns_are_bad_hellos() {
    let short = TOKEN.get(..63).unwrap();
    let long = format!("{TOKEN}0");
    let upper = TOKEN.to_ascii_uppercase();
    let not_hex = TOKEN.replace('a', "g");
    for token in [short, long.as_str(), upper.as_str(), not_hex.as_str(), ""] {
        let line = hello(&format!(
            r#""clauseo_relay":1,"token":"{token}","connector":"c""#
        ));
        assert!(parse(&line).is_err(), "{token}");
    }
    let too_long = "a".repeat(33);
    for name in [
        "",
        "-a",
        "A",
        "a_b",
        "a b",
        too_long.as_str(),
        "case\\u002dlaw",
    ] {
        let line = hello(&format!(
            r#""clauseo_relay":1,"token":"{TOKEN}","connector":"{name}""#
        ));
        assert!(parse(&line).is_err(), "{name}");
    }
    let longest = "a".repeat(32);
    let line = hello(&format!(
        r#""clauseo_relay":1,"token":"{TOKEN}","connector":"{longest}""#
    ));
    assert!(parse(&line).is_ok());
}

#[test]
fn the_line_reader_stops_at_the_cap_without_reading_on() {
    let mut reader = Cursor::new(b"abc\ndefgh\nij".to_vec());
    assert_eq!(read_line(&mut reader, 4), Line::Text(b"abc".to_vec()));
    assert_eq!(read_line(&mut reader, 4), Line::TooLong);
    // It stopped inside the long line.
    assert!(reader.position() < 10);
    let mut reader = Cursor::new(b"ab".to_vec());
    assert_eq!(read_line(&mut reader, 4), Line::End, "no newline, no line");
    let mut reader = Cursor::new(b"abcd\n".to_vec());
    assert_eq!(read_line(&mut reader, 4), Line::Text(b"abcd".to_vec()));
}
