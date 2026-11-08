//! `read_line`'s splitting, limits and overflow modes.

use std::io::Cursor;

use super::{Line, Overflow, read_line};

fn lines(input: &[u8], max: usize) -> Vec<(Line, Vec<u8>)> {
    lines_with(input, max, Overflow::Skip)
}

fn lines_with(input: &[u8], max: usize, overflow: Overflow) -> Vec<(Line, Vec<u8>)> {
    let mut input = std::io::BufReader::with_capacity(4, Cursor::new(input.to_vec()));
    let mut buf = Vec::new();
    let mut found = Vec::new();
    loop {
        let line = read_line(&mut input, &mut buf, max, overflow).unwrap();
        if line == Line::Eof {
            return found;
        }
        let stop = overflow == Overflow::Stop && line == Line::Oversize;
        found.push((line, buf.clone()));
        if stop {
            return found;
        }
    }
}

#[test]
fn lines_split_on_newline_across_reads() {
    let found = lines(b"abcdefgh\n\nxyz", 100);
    assert_eq!(
        found,
        [
            (Line::Whole, b"abcdefgh".to_vec()),
            (Line::Whole, Vec::new()),
            (Line::Partial, b"xyz".to_vec()),
        ]
    );
}

#[test]
fn stop_returns_at_the_limit_without_reading_on() {
    let mut input =
        std::io::BufReader::with_capacity(4, Cursor::new(b"ok\n123456789\nmore".to_vec()));
    let mut buf = Vec::new();
    assert_eq!(
        read_line(&mut input, &mut buf, 4, Overflow::Stop).unwrap(),
        Line::Whole
    );
    assert_eq!(
        read_line(&mut input, &mut buf, 4, Overflow::Stop).unwrap(),
        Line::Oversize
    );
    assert_eq!(buf, b"");
    let mut rest = Vec::new();
    std::io::Read::read_to_end(&mut input, &mut rest).unwrap();
    assert!(rest.ends_with(b"\nmore") && rest.len() < b"123456789\nmore".len());
    assert_eq!(lines_with(b"1234\n12345", 4, Overflow::Stop).len(), 2);
}

#[test]
fn a_line_over_the_limit_is_skipped_whole() {
    let found = lines(b"12345\n1234\n123456789\nok", 4);
    assert_eq!(
        found,
        [
            (Line::Oversize, Vec::new()),
            (Line::Whole, b"1234".to_vec()),
            (Line::Oversize, Vec::new()),
            (Line::Partial, b"ok".to_vec()),
        ]
    );
    assert_eq!(lines(b"123456789", 4), [(Line::Oversize, Vec::new())]);
}
