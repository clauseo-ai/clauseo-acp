//! The CA bundle's pure part: the DER reader, the filter, the PEM and the descriptor, on every OS.
//! The fixtures are a self-signed root, an intermediate it signed, and another self-signed root
//! (P-256, made with OpenSSL for these tests), written as hex DER.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;

use super::{
    Filtered, der, file_sddl, filter_roots, is_disallowed, lists_server_auth, path, pem_block,
};
use crate::windows::sids::Sid;

/// A self-signed root, `CN=Clauseo Test root`.
pub(super) const ROOT: &str = "\
    3082016d30820114a003020102021418d25bd79eeb5baf17301509df222aeccd4a3860300a06082a8648ce3d\
    040302301c311a301806035504030c11436c617573656f205465737420726f6f743020170d32363131303730\
    35323834325a180f32313236313031343035323834325a301c311a301806035504030c11436c617573656f20\
    5465737420726f6f743059301306072a8648ce3d020106082a8648ce3d03010703420004c58cdba9d2ec22f3\
    752713d42aa84e117e6e1f16973444b8e59773cb7f0a8f65e59c82d781abf502daa296515337de2538846011\
    bbf44d0cdef575b4d85963aaa3323030301d0603551d0e0416041491b3cb6ff06156a084b76d04d38cb88362\
    d4c410300f0603551d130101ff040530030101ff300a06082a8648ce3d040302034700304402207b45dba402\
    522bdaf7c887573ad87ae82321352d2a1085b286ead565f42a363a02207c211bc644f0d3b82e1e5d70ab5bea\
    208fbaedf9ff3cc6770be5ed24f6bca5d8";

/// `CN=Clauseo Test intermediate`, issued by [`ROOT`].
pub(super) const INTERMEDIATE: &str = "\
    308201553081fda003020102021458e9e19e282305d5eb02a6041e57340aa3cc450c300a06082a8648ce3d04\
    0302301c311a301806035504030c11436c617573656f205465737420726f6f743020170d3236313130373035\
    323834325a180f32313236313031343035323834325a30243122302006035504030c19436c617573656f2054\
    65737420696e7465726d6564696174653059301306072a8648ce3d020106082a8648ce3d0301070342000492\
    38b8aac096178fcca8d68e8b5962aed5b4c841ddb4dd5b4d85ac935cdb88781d4233bfdc472cb0478504a730\
    caa94cb2153960df7e5cb65252faa79ab58dcea3133011300f0603551d130101ff040530030101ff300a0608\
    2a8648ce3d04030203470030440220297cee92e573ace0cf6c4abc4178479681e795db65ed7f286a0a69b48d\
    4461b402207189a69e8d4d4b2fd924b7181200b9c5c2aa736397b216c76bada2be30bee10a";

/// Another self-signed root, `CN=Clauseo Test other`.
pub(super) const OTHER: &str = "\
    3082016f30820116a00302010202141a9565ea60b78337a650a75ded4d990e1d9dff4c300a06082a8648ce3d\
    040302301d311b301906035504030c12436c617573656f2054657374206f746865723020170d323631313037\
    3035323834325a180f32313236313031343035323834325a301d311b301906035504030c12436c617573656f\
    2054657374206f746865723059301306072a8648ce3d020106082a8648ce3d03010703420004a04c84e32616\
    3b4d02f8599687d4b0b1ace6b0adbb8fc273f51dd31482a858bbed16d37c64bfdd6a51bc9d3cb0454e75bd17\
    02b6fe5b43a4b0fbf96c4c81d029a3323030301d0603551d0e04160414b91a29690aab121112a9bbb8df5ef4\
    fef89ccb61300f0603551d130101ff040530030101ff300a06082a8648ce3d040302034700304402200f2040\
    3d6a093eae0c267c4cc665507b8386dc80c803e3e5d4007b83f9020aec02207dfe187a24f557a118e0a8bb12\
    6313d997d0aa5a5652f1c920720d71466926be";

/// The DER of every `CERTIFICATE` block in `pem`.
pub(super) fn ders(pem: &str) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut body: Option<String> = None;
    for line in pem.lines() {
        match line {
            "-----BEGIN CERTIFICATE-----" => body = Some(String::new()),
            "-----END CERTIFICATE-----" => out.push(STANDARD.decode(body.take().unwrap()).unwrap()),
            text => body.as_mut().unwrap().push_str(text),
        }
    }
    assert!(body.is_none(), "an unterminated block");
    out
}

/// The DER that the fixture `hex` (lowercase hex, no spaces) spells.
pub(super) fn der_of(hex: &str) -> Vec<u8> {
    assert_eq!(hex.len() % 2, 0);
    hex.as_bytes()
        .chunks(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

/// The PEM a bundle holding only `hex`'s certificate is.
pub(super) fn pem_of(hex: &str) -> String {
    pem_block(&der_of(hex))
}

/// [`ROOT`] with one byte of its serial number changed: another DER, the same public key.
fn root_reissued() -> Vec<u8> {
    let mut der = der_of(ROOT);
    // `[0] { INTEGER 2 }`, then the serial's INTEGER tag and length: its first byte follows.
    let version = [0xA0, 0x03, 0x02, 0x01, 0x02, 0x02];
    let at = der
        .windows(version.len())
        .position(|w| w == version)
        .unwrap();
    der[at + 7] ^= 0x01;
    der
}

fn sid(text: &str) -> Sid {
    Sid::parse(text).unwrap()
}

#[test]
fn the_reader_finds_the_names_and_the_key() {
    let root = der_of(ROOT);
    let parts = der::certificate(&root).unwrap();
    assert_eq!(parts.issuer, parts.subject);
    let intermediate = der_of(INTERMEDIATE);
    let int_parts = der::certificate(&intermediate).unwrap();
    assert_eq!(int_parts.issuer, parts.subject, "issued by the root");
    assert_ne!(int_parts.issuer, int_parts.subject);
    // Each part is a whole SEQUENCE TLV.
    for tlv in [parts.issuer, parts.subject, parts.spki] {
        let (read, rest) = der::read(tlv).unwrap();
        assert_eq!((read.tag, read.raw, rest.len()), (0x30, tlv, 0));
    }
}

#[test]
fn the_reader_refuses_what_is_not_der() {
    let root = der_of(ROOT);
    // Every truncation, and one byte too many.
    for end in 0..root.len() {
        assert!(der::certificate(&root[..end]).is_none(), "{end}");
    }
    let mut longer = root;
    longer.push(0);
    assert!(der::certificate(&longer).is_none());
    // The indefinite form, a long form for a short length, a leading zero, five length bytes,
    // a high tag number, a length past the end.
    for bad in [
        &[0x30, 0x80, 0x00, 0x00][..],
        &[0x30, 0x81, 0x01, 0x00],
        &[0x30, 0x82, 0x00, 0x81],
        &[0x30, 0x85, 0x01, 0x00, 0x00, 0x00, 0x00],
        &[0x1F, 0x01, 0x00],
        &[0x30, 0x02, 0x00],
    ] {
        assert!(der::read(bad).is_none(), "{bad:02x?}");
    }
    assert_eq!(der::read(&[0x05, 0x00]).unwrap().0.raw, &[0x05, 0x00]);
}

#[test]
fn the_root_is_kept_and_the_intermediate_dropped() {
    let filtered = filter_roots(&[der_of(ROOT), der_of(INTERMEDIATE)], &[]);
    assert_eq!(
        (filtered.kept, filtered.dropped_not_self_issued),
        (1, 1),
        "{filtered:?}"
    );
    assert_eq!(filtered.pem, pem_of(ROOT));
}

#[test]
fn a_root_also_in_disallowed_is_dropped() {
    let filtered = filter_roots(&[der_of(ROOT), der_of(OTHER)], &[der_of(OTHER)]);
    assert_eq!(
        Filtered {
            pem: pem_of(ROOT),
            kept: 1,
            dropped_disallowed: 1,
            dropped_not_self_issued: 0,
            dropped_unparsable: 0,
            dropped_limited: 0,
        },
        filtered
    );
}

#[test]
fn a_disallowed_certificate_with_the_roots_key_drops_the_root() {
    let reissued = root_reissued();
    assert_ne!(reissued, der_of(ROOT));
    let root = der_of(ROOT);
    let parts = der::certificate(&root).unwrap();
    assert_eq!(der::certificate(&reissued).unwrap().spki, parts.spki);
    let filtered = filter_roots(&[der_of(ROOT), der_of(OTHER)], &[reissued]);
    assert_eq!((filtered.kept, filtered.dropped_disallowed), (1, 1));
    assert_eq!(filtered.pem, pem_of(OTHER));
}

#[test]
fn the_matcher_compares_whole_der_or_the_key() {
    let banned_der = [&b"cert"[..]].into_iter().collect();
    let banned_spki = [&b"key"[..]].into_iter().collect();
    assert!(is_disallowed(
        b"cert",
        b"other key",
        &banned_der,
        &banned_spki
    ));
    assert!(is_disallowed(
        b"other cert",
        b"key",
        &banned_der,
        &banned_spki
    ));
    assert!(!is_disallowed(b"cer", b"ke", &banned_der, &banned_spki));
}

#[test]
fn duplicates_collapse_and_the_output_is_sorted() {
    let one = filter_roots(&[der_of(OTHER), der_of(ROOT), der_of(OTHER)], &[]);
    let two = filter_roots(&[der_of(ROOT), der_of(OTHER), der_of(ROOT)], &[]);
    assert_eq!(one, two);
    assert_eq!(one.kept, 2);
    let mut expected = vec![der_of(ROOT), der_of(OTHER)];
    expected.sort();
    assert_eq!(ders(&one.pem), expected);
}

#[test]
fn garbage_and_truncated_der_are_dropped_as_unparsable() {
    let root = der_of(ROOT);
    let filtered = filter_roots(
        &[
            Vec::new(),
            b"not a certificate".to_vec(),
            root[..root.len() / 2].to_vec(),
            vec![0x30, 0x80, 0x00, 0x00],
            root.clone(),
        ],
        &[b"garbage".to_vec()],
    );
    assert_eq!((filtered.kept, filtered.dropped_unparsable), (1, 4));
    assert_eq!(ders(&filtered.pem), vec![root]);
}

#[test]
fn the_pem_has_64_column_lines_and_decodes_to_the_kept_certificates() {
    let filtered = filter_roots(&[der_of(ROOT), der_of(OTHER), der_of(INTERMEDIATE)], &[]);
    assert!(filtered.pem.lines().all(|line| line.len() <= 64));
    assert!(!filtered.pem.contains('\r'));
    assert!(filtered.pem.ends_with("-----END CERTIFICATE-----\n"));
    let mut expected = vec![der_of(ROOT), der_of(OTHER)];
    expected.sort();
    assert_eq!(ders(&filtered.pem), expected);
    assert_eq!(filter_roots(&[], &[]), Filtered::default());
}

#[test]
fn the_file_descriptor_is_exact() {
    let desktop = sid("S-1-5-21-1-2-3-1001");
    let pool = [
        sid("S-1-5-21-1-2-3-1102"),
        sid("S-1-5-21-1-2-3-1101"),
        sid("S-1-5-21-1-2-3-1102"),
    ];
    assert_eq!(
        file_sddl(&desktop, &pool),
        "O:S-1-5-21-1-2-3-1001D:P(A;;FA;;;S-1-5-21-1-2-3-1001)(A;;FA;;;SY)\
         (A;;FR;;;S-1-5-21-1-2-3-1101)(A;;FR;;;S-1-5-21-1-2-3-1102)"
    );
    assert_eq!(
        file_sddl(&desktop, &[]),
        "O:S-1-5-21-1-2-3-1001D:P(A;;FA;;;S-1-5-21-1-2-3-1001)(A;;FA;;;SY)"
    );
}

#[test]
fn the_bundle_lives_in_its_own_folder() {
    let data = std::path::Path::new("data");
    assert_eq!(path(data), data.join("sandbox-ca").join("roots.pem"));
}

/// `SEQUENCE OF OBJECT IDENTIFIER` for server authentication and code signing.
pub(super) const SERVER_AND_CODE: &[u8] = &[
    0x30, 0x14, 0x06, 0x08, 0x2B, 0x06, 0x01, 0x05, 0x05, 0x07, 0x03, 0x01, 0x06, 0x08, 0x2B, 0x06,
    0x01, 0x05, 0x05, 0x07, 0x03, 0x03,
];
/// The same for client authentication alone.
pub(super) const CLIENT_ONLY: &[u8] = &[
    0x30, 0x0A, 0x06, 0x08, 0x2B, 0x06, 0x01, 0x05, 0x05, 0x07, 0x03, 0x02,
];
/// The same for server authentication alone.
pub(super) const SERVER_ONLY: &[u8] = &[
    0x30, 0x0A, 0x06, 0x08, 0x2B, 0x06, 0x01, 0x05, 0x05, 0x07, 0x03, 0x01,
];

#[test]
fn a_usage_list_reads_whole_or_not_at_all() {
    assert_eq!(lists_server_auth(SERVER_AND_CODE), Some(true));
    assert_eq!(lists_server_auth(SERVER_ONLY), Some(true));
    assert_eq!(lists_server_auth(CLIENT_ONLY), Some(false));
    assert_eq!(lists_server_auth(&[0x30, 0x00]), Some(false));
    for bad in [
        &[][..],
        &[0x30],
        &[0x30, 0x02, 0x06, 0x00],
        &[0x30, 0x02, 0x04, 0x00],
        &[0x30, 0x00, 0x00],
        &SERVER_ONLY[..SERVER_ONLY.len() - 1],
    ] {
        assert_eq!(lists_server_auth(bad), None, "{bad:02x?}");
    }
}
