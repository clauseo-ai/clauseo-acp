//! The body, before the schema sees it: the two headers the Worker reads, a read that stops one
//! byte past the gzip cap, and an inflate that stops one byte past the JSON cap. Any failure is
//! [`Refusal::Bad`], answered `400` like a schema failure, so a refusal says nothing about why.

use std::io::Read;

use flate2::read::GzDecoder;

use crate::route::Refusal;

/// The body must be gzip and, when its length is given, within `cap`. `encoding` and `length`
/// are the raw `Content-Encoding` and `Content-Length` values. Without `Content-Length` (a
/// chunked body) the cap is still kept by [`Limited`].
pub(crate) fn check_headers(
    encoding: Option<&str>,
    length: Option<&str>,
    cap: usize,
) -> Result<(), Refusal> {
    if !encoding.is_some_and(|value| value.eq_ignore_ascii_case("gzip")) {
        return Err(Refusal::Bad);
    }
    if let Some(length) = length {
        let length: u64 = length.parse()?;
        if length > u64::try_from(cap)? {
            return Err(Refusal::Bad);
        }
    }
    Ok(())
}

/// The body's bytes as they arrive, refused as soon as they pass `cap`, so a sender cannot make
/// the Worker hold more than the cap plus one chunk.
#[derive(Debug)]
pub(crate) struct Limited {
    cap: usize,
    bytes: Vec<u8>,
}

impl Limited {
    /// An empty body that may grow to `cap` bytes.
    pub(crate) fn new(cap: usize) -> Limited {
        Limited {
            cap,
            bytes: Vec::new(),
        }
    }

    /// Adds the next chunk, or refuses when the body would pass the cap.
    pub(crate) fn push(&mut self, chunk: &[u8]) -> Result<(), Refusal> {
        if chunk.len() > self.cap - self.bytes.len() {
            return Err(Refusal::Bad);
        }
        self.bytes.extend_from_slice(chunk);
        Ok(())
    }

    /// The whole body.
    pub(crate) fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
}

/// The JSON inside a gzip body, refused when it is not gzip or would pass `cap`. The decoder
/// reads at most `cap + 1` bytes, so a small body that inflates to gigabytes costs only that.
pub(crate) fn inflate(gzip: &[u8], cap: usize) -> Result<Vec<u8>, Refusal> {
    let limit = u64::try_from(cap)?.saturating_add(1);
    let mut json = Vec::new();
    GzDecoder::new(gzip).take(limit).read_to_end(&mut json)?;
    if json.len() > cap {
        return Err(Refusal::Bad);
    }
    Ok(json)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use flate2::Compression;
    use flate2::write::GzEncoder;

    use super::*;

    fn gzip(bytes: &[u8]) -> Vec<u8> {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(bytes).unwrap();
        encoder.finish().unwrap()
    }

    #[test]
    fn the_body_must_say_gzip_and_fit_its_length() {
        assert_eq!(check_headers(Some("gzip"), Some("100"), 16_000), Ok(()));
        assert_eq!(check_headers(Some("GZip"), None, 16_000), Ok(()));
        assert_eq!(check_headers(Some("gzip"), Some("16000"), 16_000), Ok(()));
        for (encoding, length) in [
            (None, Some("100")),
            (Some("identity"), Some("100")),
            (Some("br"), Some("100")),
            (Some("gzip, br"), Some("100")),
            (Some(" gzip"), Some("100")),
            (Some("gzip"), Some("16001")),
            (Some("gzip"), Some("-1")),
            (Some("gzip"), Some("1e3")),
            (Some("gzip"), Some("")),
        ] {
            assert_eq!(
                check_headers(encoding, length, 16_000),
                Err(Refusal::Bad),
                "{encoding:?} {length:?}"
            );
        }
    }

    #[test]
    fn a_body_past_its_cap_is_refused_at_the_chunk_that_passes_it() {
        let mut body = Limited::new(10);
        body.push(b"12345").unwrap();
        body.push(b"67890").unwrap();
        assert_eq!(body.push(b"x"), Err(Refusal::Bad));
        assert_eq!(body.into_bytes(), b"1234567890");

        let mut body = Limited::new(10);
        body.push(b"123456").unwrap();
        assert_eq!(body.push(b"78901"), Err(Refusal::Bad));
        assert_eq!(Limited::new(4_000).push(&vec![0; 4_001]), Err(Refusal::Bad));
    }

    #[test]
    fn inflating_stops_one_byte_past_the_cap() {
        let json = br#"{ "days": [] }"#;
        assert_eq!(inflate(&gzip(json), 64_000), Ok(json.to_vec()));
        assert_eq!(
            inflate(&gzip(&vec![b' '; 64_000]), 64_000).map(|j| j.len()),
            Ok(64_000)
        );
        assert_eq!(
            inflate(&gzip(&vec![b' '; 64_001]), 64_000),
            Err(Refusal::Bad)
        );
        // About 10 KB of gzip that would inflate to 10 MB.
        let bomb = gzip(&vec![b' '; 10_000_000]);
        assert!(bomb.len() < 16_000);
        assert_eq!(inflate(&bomb, 64_000), Err(Refusal::Bad));
    }

    #[test]
    fn a_body_that_is_not_gzip_is_refused() {
        assert_eq!(inflate(br#"{"days":[]}"#, 64_000), Err(Refusal::Bad));
        assert_eq!(inflate(b"", 64_000), Err(Refusal::Bad));
        let mut cut = gzip(b"{\"days\":[]}");
        cut.truncate(cut.len() - 4);
        assert_eq!(inflate(&cut, 64_000), Err(Refusal::Bad));
    }
}
