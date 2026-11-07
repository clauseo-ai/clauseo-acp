//! A minimal DER reader: just enough of an X.509 certificate to find its issuer, subject and
//! public key. Definite lengths only, in their shortest form; single-byte tags only; anything
//! else, or any length past its container, is refused (`None`), never a panic.

/// `SEQUENCE`.
const SEQUENCE: u8 = 0x30;
/// `INTEGER`.
const INTEGER: u8 = 0x02;
/// `BIT STRING`.
const BIT_STRING: u8 = 0x03;
/// `OBJECT IDENTIFIER`.
const OID: u8 = 0x06;
/// `[0] EXPLICIT`, the version.
const VERSION: u8 = 0xA0;

/// One TLV: its tag, the whole encoding, and the content inside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Tlv<'a> {
    /// The tag byte.
    pub(super) tag: u8,
    /// Tag, length and content, as encoded.
    pub(super) raw: &'a [u8],
    /// The content alone.
    pub(super) content: &'a [u8],
}

/// The parts of a certificate the filter compares, each a whole TLV as encoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Parts<'a> {
    /// The issuer `Name`.
    pub(super) issuer: &'a [u8],
    /// The subject `Name`.
    pub(super) subject: &'a [u8],
    /// The `SubjectPublicKeyInfo`.
    pub(super) spki: &'a [u8],
}

/// The first TLV of `input` and what follows it.
pub(super) fn read(input: &[u8]) -> Option<(Tlv<'_>, &[u8])> {
    let (&tag, rest) = input.split_first()?;
    // The high-tag-number form: no certificate field this reads uses it.
    if tag & 0x1F == 0x1F {
        return None;
    }
    let (&first, mut rest) = rest.split_first()?;
    let length = if first < 0x80 {
        usize::from(first)
    } else {
        // 0x80 is the indefinite form; more than four length bytes is no certificate.
        let count = usize::from(first & 0x7F);
        if count == 0 || count > 4 || rest.len() < count {
            return None;
        }
        let (bytes, after) = rest.split_at(count);
        rest = after;
        if bytes.first() == Some(&0) {
            return None;
        }
        let length = bytes
            .iter()
            .fold(0usize, |total, &byte| (total << 8) | usize::from(byte));
        // The long form for what the short form holds is not DER.
        if length < 0x80 {
            return None;
        }
        length
    };
    if rest.len() < length {
        return None;
    }
    let (content, after) = rest.split_at(length);
    let header = input.len() - rest.len();
    let raw = input.get(..header + length)?;
    Some((Tlv { tag, raw, content }, after))
}

/// The first TLV of `input`, which must have the tag `tag`, and what follows it.
fn expect(input: &[u8], tag: u8) -> Option<(Tlv<'_>, &[u8])> {
    read(input).filter(|(tlv, _)| tlv.tag == tag)
}

/// The issuer, subject and public key of the DER certificate `der`, which must be exactly one
/// `Certificate ::= SEQUENCE { tbsCertificate, signatureAlgorithm, signature }` with nothing
/// after it, its `tbsCertificate` starting `[0] version OPTIONAL, serialNumber, signature,
/// issuer, validity, subject, subjectPublicKeyInfo` and every field after those a whole TLV.
pub(super) fn certificate(der: &[u8]) -> Option<Parts<'_>> {
    let (outer, after) = expect(der, SEQUENCE)?;
    if !after.is_empty() {
        return None;
    }
    let (tbs, rest) = expect(outer.content, SEQUENCE)?;
    let (_, rest) = expect(rest, SEQUENCE)?;
    let (_, rest) = expect(rest, BIT_STRING)?;
    if !rest.is_empty() {
        return None;
    }
    let mut fields = tbs.content;
    if let Some((version, rest)) = expect(fields, VERSION) {
        let (_, inner) = expect(version.content, INTEGER)?;
        if !inner.is_empty() {
            return None;
        }
        fields = rest;
    }
    let (_, rest) = expect(fields, INTEGER)?;
    let (_, rest) = expect(rest, SEQUENCE)?;
    let (issuer, rest) = expect(rest, SEQUENCE)?;
    let (_, rest) = expect(rest, SEQUENCE)?;
    let (subject, rest) = expect(rest, SEQUENCE)?;
    let (spki, mut rest) = expect(rest, SEQUENCE)?;
    while !rest.is_empty() {
        rest = read(rest)?.1;
    }
    Some(Parts {
        issuer: issuer.raw,
        subject: subject.raw,
        spki: spki.raw,
    })
}

/// The content of each OID in the DER `SEQUENCE OF OBJECT IDENTIFIER` `encoded`, which must
/// be exactly that with nothing after it; an empty sequence gives none.
pub(super) fn oid_list(encoded: &[u8]) -> Option<Vec<&[u8]>> {
    let (list, after) = expect(encoded, SEQUENCE)?;
    if !after.is_empty() {
        return None;
    }
    let mut rest = list.content;
    let mut out = Vec::new();
    while !rest.is_empty() {
        let (oid, next) = expect(rest, OID)?;
        if oid.content.is_empty() {
            return None;
        }
        out.push(oid.content);
        rest = next;
    }
    Some(out)
}
