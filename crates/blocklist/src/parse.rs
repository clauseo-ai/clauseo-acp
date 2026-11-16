//! Reading the list's text (after gunzip): comment lines, then `<host> <tag>` lines sorted
//! byte-wise by host with no duplicates, as many as the trusted comment says.

use std::io::{BufRead, BufReader, Read};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use crate::list::{BlockList, Builder, Tag};
use crate::{Error, Header};

/// The most entries a list may have.
pub const MAX_ENTRIES: u32 = 2_000_000;

/// The most bytes a list may have decompressed: 64 MiB.
pub const MAX_TEXT: u64 = 64 * 1024 * 1024;

/// The longest host name DNS carries, without the trailing dot.
const NAME_MAX: usize = 253;
/// The longest label.
const LABEL_MAX: usize = 63;

/// Decompresses and parses a list whose signature [`verify`](crate::verify) checked. Reads at
/// most [`MAX_TEXT`] bytes: a gzip bomb stops there.
pub fn parse_gz(gz: &[u8], header: Header) -> Result<BlockList, Error> {
    let decoder = flate2::read::GzDecoder::new(gz).take(MAX_TEXT + 1);
    parse_from(BufReader::with_capacity(64 * 1024, decoder), header)
}

/// Parses a list's text, as [`parse_gz`] does after gunzip.
pub fn parse_plain(text: &[u8], header: Header) -> Result<BlockList, Error> {
    if text.len() as u64 > MAX_TEXT {
        return Err(Error::TooBig);
    }
    parse_from(text, header)
}

fn parse_from(mut reader: impl BufRead, header: Header) -> Result<BlockList, Error> {
    if header.entries > MAX_ENTRIES {
        return Err(Error::TooBig);
    }
    let mut builder = Builder::with_capacity(header.entries as usize);
    let (mut raw, mut previous) = (Vec::with_capacity(256), Vec::<u8>::with_capacity(256));
    let (mut read, mut count, mut number) = (0u64, 0u32, 0usize);
    loop {
        raw.clear();
        let got = reader.read_until(b'\n', &mut raw).ok().ok_or(Error::Gzip)?;
        if got == 0 {
            break;
        }
        read += got as u64;
        if read > MAX_TEXT {
            return Err(Error::TooBig);
        }
        number += 1;
        let line = raw.strip_suffix(b"\n").unwrap_or(&raw);
        if line.first() == Some(&b'#') {
            continue;
        }
        let malformed = Error::Malformed { line: number };
        let (host, tag) = split(line).ok_or(malformed.clone())?;
        if previous.as_slice() >= host && count > 0 {
            return Err(if previous.as_slice() == host {
                Error::Duplicate { line: number }
            } else {
                Error::Unsorted { line: number }
            });
        }
        let stored = stored(host).ok_or(malformed)?;
        count += 1;
        if count > header.entries {
            return Err(Error::EntriesMismatch {
                said: header.entries,
                found: count,
            });
        }
        if !builder.push(&stored, tag) {
            return Err(Error::TooBig);
        }
        previous.clear();
        previous.extend_from_slice(host);
    }
    if count != header.entries {
        return Err(Error::EntriesMismatch {
            said: header.entries,
            found: count,
        });
    }
    Ok(builder.finish(header.generated))
}

/// `<host> <tag>`: one space, a tag letter, nothing else.
fn split(line: &[u8]) -> Option<(&[u8], Tag)> {
    let space = line.iter().position(|b| *b == b' ')?;
    let (host, rest) = line.split_at(space);
    let tag = Tag::from_letter(rest.get(1..)?)?;
    (!host.is_empty()).then_some((host, tag))
}

/// The entry's stored form: a domain as listed (checked: lowercase ASCII labels of 1 to 63 of
/// `a-z`, `0-9`, `-` and `_`, not starting or ending with `-`, at least two labels, a last label
/// that is not all digits), or an IP literal (dotted IPv4, or IPv6 in brackets) as `[address]`.
fn stored(host: &[u8]) -> Option<String> {
    let host = std::str::from_utf8(host).ok()?;
    if let Some(inner) = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')) {
        let ip = inner.parse::<Ipv6Addr>().ok()?;
        return Some(format!("[{}]", IpAddr::V6(ip).to_canonical()));
    }
    if let Ok(ip) = host.parse::<Ipv4Addr>() {
        return Some(format!("[{ip}]"));
    }
    if host.len() > NAME_MAX {
        return None;
    }
    let mut labels = 0;
    for label in host.split('.') {
        let ok = (1..=LABEL_MAX).contains(&label.len())
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_');
        if !ok {
            return None;
        }
        labels += 1;
    }
    let last = host.rsplit('.').next()?;
    (labels >= 2 && !last.bytes().all(|b| b.is_ascii_digit())).then(|| host.to_owned())
}
