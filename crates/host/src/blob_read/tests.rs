use clauseo_protocol::{Count, MAX_BLOB_READ_BYTES, ReadLen};

use super::{cut, request_len};

/// What the store returns: up to `len` bytes from `offset`, cut at the blob's end.
fn read(blob: &[u8], offset: u64, len: u32) -> &[u8] {
    let start = usize::try_from(offset).unwrap().min(blob.len());
    let end = (start + len as usize).min(blob.len());
    &blob[start..end]
}

fn read_blob(blob: &[u8], offset: u64, len: u32) -> clauseo_protocol::BlobRange {
    let len = ReadLen::new(len).unwrap();
    let bytes = read(blob, offset, request_len(len));
    cut(Count::new(offset).unwrap(), len, bytes).unwrap()
}

#[test]
fn read_blob_cuts_to_whole_characters() {
    // "a😀b": the emoji is bytes 1..5.
    let blob = "a😀b😀".as_bytes();

    // An offset inside a 4-byte character starts at the next character.
    let range = read_blob(blob, 2, 10);
    assert_eq!(range.offset.get(), 5);
    assert_eq!(range.text, "b😀");
    assert_eq!(range.end.get(), 10);

    // A len of 1 at a 4-byte character returns that character.
    let range = read_blob(blob, 1, 1);
    assert_eq!((range.offset.get(), range.text.as_str()), (1, "😀"));
    assert_eq!(range.end.get(), 5);

    // A 1 MiB read from inside a character, where the cap ends inside a later character.
    let mut text = String::from("a");
    while text.len() < 2 * MAX_BLOB_READ_BYTES as usize {
        text.push('😀');
    }
    let blob = text.as_bytes();
    let cap = MAX_BLOB_READ_BYTES;
    assert_eq!(request_len(ReadLen::new(cap).unwrap()), cap);
    let first = read_blob(blob, 2, cap);
    assert_eq!(first.offset.get(), 5);
    // The cap is read from offset 2, so it ends at byte 2 + 1 MiB, inside a character.
    let cap_end = 2 + u64::from(cap);
    assert_ne!((cap_end - 1) % 4, 0);
    assert!(first.end.get() < cap_end && cap_end - first.end.get() < 4);
    assert_eq!((first.end.get() - 1) % 4, 0);
    assert!(first.text.chars().all(|c| c == '😀'));
    // The next read from `end` starts with the cut character: no byte missing or repeated.
    let second = read_blob(blob, first.end.get(), 8);
    assert_eq!(second.offset, first.end);
    assert_eq!(second.text, "😀😀");
    let joined = format!("a{}{}", first.text, second.text);
    assert_eq!(joined.as_bytes(), &blob[..joined.len()]);
}

#[test]
fn read_at_the_end_is_empty() {
    let range = read_blob(b"abc", 3, 5);
    assert_eq!((range.offset.get(), range.end.get()), (3, 3));
    assert_eq!(range.text, "");
}

#[test]
fn bytes_that_are_not_utf8_are_corrupt() {
    let error = cut(Count::ZERO, ReadLen::new(4).unwrap(), b"ab\xffcd").unwrap_err();
    assert_eq!(error.code(), clauseo_protocol::ErrorCode::StoreCorrupt);
}
