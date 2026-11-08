//! The refresh tests' signer: an Ed25519 key made in the test (ring), BLAKE2b-512 (RFC 7693,
//! written out here, since nothing in the tree exports one), and the minisign and Tauri forms of
//! its public key and signatures, as `tauri signer` writes them.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use ring::rand::SystemRandom;
use ring::signature::{Ed25519KeyPair, KeyPair as _};

/// A minisign key made for one test.
pub(super) struct TestKey {
    pair: Ed25519KeyPair,
    id: [u8; 8],
}

impl TestKey {
    /// A fresh key whose id prints as `id` (minisign's little-endian hex).
    pub(super) fn new(id: u64) -> TestKey {
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).expect("a new key");
        let pair = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).expect("the new key");
        TestKey {
            pair,
            id: id.to_le_bytes(),
        }
    }

    /// The public key in Tauri's form: base64 of the minisign `.pub` file.
    pub(super) fn public(&self) -> String {
        let mut line = b"Ed".to_vec();
        line.extend_from_slice(&self.id);
        line.extend_from_slice(self.pair.public_key().as_ref());
        let id = u64::from_le_bytes(self.id);
        let file = format!(
            "untrusted comment: minisign public key: {id:016X}\n{}\n",
            STANDARD.encode(line)
        );
        STANDARD.encode(file)
    }

    /// A prehashed (`ED`) signature of `data` with the trusted comment `comment`, in Tauri's
    /// form: base64 of the minisign `.sig` file.
    pub(super) fn sign(&self, data: &[u8], comment: &str) -> Vec<u8> {
        self.sign_as(*b"ED", &blake2b512(data), comment)
    }

    /// A legacy (`Ed`) signature of `data` itself.
    pub(super) fn sign_legacy(&self, data: &[u8], comment: &str) -> Vec<u8> {
        self.sign_as(*b"Ed", data, comment)
    }

    /// The `.sig` for `algorithm` over `signed`.
    fn sign_as(&self, algorithm: [u8; 2], signed: &[u8], comment: &str) -> Vec<u8> {
        let signature = self.pair.sign(signed);
        let mut line = algorithm.to_vec();
        line.extend_from_slice(&self.id);
        line.extend_from_slice(signature.as_ref());
        let mut global = signature.as_ref().to_vec();
        global.extend_from_slice(comment.as_bytes());
        let global = self.pair.sign(&global);
        let file = format!(
            "untrusted comment: signature from tauri secret key\n{}\n\
             trusted comment: {comment}\n{}\n",
            STANDARD.encode(line),
            STANDARD.encode(global.as_ref())
        );
        STANDARD.encode(file).into_bytes()
    }
}

/// The trusted comment `tauri signer` writes, for `file` at `version`.
pub(super) fn comment(file: &str, version: &str) -> String {
    format!("timestamp:1762500000\tfile:{file}\tversion:{version}")
}

/// A minimal x64 PE32+ image whose one section, `.rsrc`, holds a `VERSIONINFO` with `version`
/// as its `FILEVERSION`, then `tail` past the section (so two images of one version differ).
pub(super) fn image(version: [u16; 4], tail: &[u8]) -> Vec<u8> {
    image_for(0x8664, version, tail)
}

/// [`image`] for the COFF `Machine` `machine`.
pub(super) fn image_for(machine: u16, version: [u16; 4], tail: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0u8; 0x200];
    let put = |bytes: &mut Vec<u8>, at: usize, value: &[u8]| {
        if bytes.len() < at + value.len() {
            bytes.resize(at + value.len(), 0);
        }
        bytes[at..at + value.len()].copy_from_slice(value);
    };
    let u16le = |v: u16| v.to_le_bytes();
    let u32le = |v: u32| v.to_le_bytes();
    put(&mut bytes, 0, b"MZ");
    put(&mut bytes, 0x3C, &u32le(0x40));
    put(&mut bytes, 0x40, b"PE\0\0");
    put(&mut bytes, 0x44, &u16le(machine));
    put(&mut bytes, 0x46, &u16le(1));
    put(&mut bytes, 0x54, &u16le(240));
    let optional = 0x58;
    put(&mut bytes, optional, &u16le(0x20B));
    put(&mut bytes, optional + 108, &u32le(16));
    // The resource section: RVA 0x1000, at 0x200 in the file.
    let mut rsrc = vec![0u8; 0x58];
    let table = |rsrc: &mut Vec<u8>, at: usize, id: u32, below: u32| {
        rsrc[at + 14..at + 16].copy_from_slice(&1u16.to_le_bytes());
        rsrc[at + 16..at + 20].copy_from_slice(&id.to_le_bytes());
        rsrc[at + 20..at + 24].copy_from_slice(&below.to_le_bytes());
    };
    table(&mut rsrc, 0, 16, 0x8000_0018);
    table(&mut rsrc, 0x18, 1, 0x8000_0030);
    table(&mut rsrc, 0x30, 0x409, 0x48);
    let mut info = Vec::new();
    info.extend_from_slice(&u16le(92));
    info.extend_from_slice(&u16le(52));
    info.extend_from_slice(&u16le(0));
    info.extend(
        "VS_VERSION_INFO\0"
            .encode_utf16()
            .flat_map(u16::to_le_bytes),
    );
    info.extend_from_slice(&[0, 0]);
    let [a, b, c, d] = version;
    for word in [
        0xFEEF_04BD,
        0x0001_0000,
        u32::from(a) << 16 | u32::from(b),
        u32::from(c) << 16 | u32::from(d),
    ] {
        info.extend_from_slice(&u32le(word));
    }
    info.resize(92, 0);
    rsrc[0x48..0x4C].copy_from_slice(&u32le(0x1058));
    rsrc[0x4C..0x50].copy_from_slice(&u32le(92));
    rsrc.extend_from_slice(&info);
    let size = u32::try_from(rsrc.len()).unwrap();
    put(&mut bytes, optional + 112 + 16, &u32le(0x1000));
    put(&mut bytes, optional + 112 + 20, &u32le(size));
    let section = optional + 240;
    put(&mut bytes, section, b".rsrc\0\0\0");
    put(&mut bytes, section + 8, &u32le(size));
    put(&mut bytes, section + 12, &u32le(0x1000));
    put(&mut bytes, section + 16, &u32le(size));
    put(&mut bytes, section + 20, &u32le(0x200));
    put(&mut bytes, 0x200, &rsrc);
    bytes.extend_from_slice(tail);
    bytes
}

/// `BLAKE2b`'s initial vector.
const IV: [u64; 8] = [
    0x6a09_e667_f3bc_c908,
    0xbb67_ae85_84ca_a73b,
    0x3c6e_f372_fe94_f82b,
    0xa54f_f53a_5f1d_36f1,
    0x510e_527f_ade6_82d1,
    0x9b05_688c_2b3e_6c1f,
    0x1f83_d9ab_fb41_bd6b,
    0x5be0_cd19_137e_2179,
];

/// `BLAKE2b`'s message schedule, rounds 0 to 9; rounds 10 and 11 repeat 0 and 1.
const SIGMA: [[usize; 16]; 10] = [
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
    [14, 10, 4, 8, 9, 15, 13, 6, 1, 12, 0, 2, 11, 7, 5, 3],
    [11, 8, 12, 0, 5, 2, 15, 13, 10, 14, 3, 6, 7, 1, 9, 4],
    [7, 9, 3, 1, 13, 12, 11, 14, 2, 6, 5, 10, 4, 0, 15, 8],
    [9, 0, 5, 7, 2, 4, 10, 15, 14, 1, 11, 12, 6, 8, 3, 13],
    [2, 12, 6, 10, 0, 11, 8, 3, 4, 13, 7, 5, 15, 14, 1, 9],
    [12, 5, 1, 15, 14, 13, 4, 10, 0, 7, 6, 3, 9, 2, 8, 11],
    [13, 11, 7, 14, 12, 1, 3, 9, 5, 0, 15, 4, 8, 6, 2, 10],
    [6, 15, 14, 9, 11, 3, 0, 8, 12, 2, 13, 7, 1, 4, 10, 5],
    [10, 2, 8, 4, 7, 6, 1, 5, 15, 11, 9, 14, 3, 12, 13, 0],
];

/// BLAKE2b-512 of `data`, unkeyed (RFC 7693).
pub(super) fn blake2b512(data: &[u8]) -> [u8; 64] {
    let mut h = IV;
    h[0] ^= 0x0101_0000 ^ 64;
    let blocks = data.len().div_ceil(128).max(1);
    for index in 0..blocks {
        let start = index * 128;
        let end = data.len().min(start + 128);
        let mut block = [0u8; 128];
        block[..end - start].copy_from_slice(&data[start..end]);
        let last = index + 1 == blocks;
        compress(
            &mut h,
            &block,
            u64::try_from(end).expect("a test's size"),
            last,
        );
    }
    let mut out = [0u8; 64];
    for (chunk, word) in out.as_chunks_mut::<8>().0.iter_mut().zip(h) {
        chunk.copy_from_slice(&word.to_le_bytes());
    }
    out
}

/// One compression of `block`, `t` bytes in so far (a test's message is under 2^64 bytes, so
/// the counter's high word stays 0).
#[expect(clippy::many_single_char_names, reason = "RFC 7693's names")]
fn compress(h: &mut [u64; 8], block: &[u8; 128], t: u64, last: bool) {
    let mut m = [0u64; 16];
    for (word, bytes) in m.iter_mut().zip(block.as_chunks::<8>().0) {
        *word = u64::from_le_bytes(*bytes);
    }
    let mut v = [0u64; 16];
    v[..8].copy_from_slice(h);
    v[8..].copy_from_slice(&IV);
    v[12] ^= t;
    if last {
        v[14] = !v[14];
    }
    for round in 0..12 {
        let s = &SIGMA[round % 10];
        g(&mut v, [0, 4, 8, 12], m[s[0]], m[s[1]]);
        g(&mut v, [1, 5, 9, 13], m[s[2]], m[s[3]]);
        g(&mut v, [2, 6, 10, 14], m[s[4]], m[s[5]]);
        g(&mut v, [3, 7, 11, 15], m[s[6]], m[s[7]]);
        g(&mut v, [0, 5, 10, 15], m[s[8]], m[s[9]]);
        g(&mut v, [1, 6, 11, 12], m[s[10]], m[s[11]]);
        g(&mut v, [2, 7, 8, 13], m[s[12]], m[s[13]]);
        g(&mut v, [3, 4, 9, 14], m[s[14]], m[s[15]]);
    }
    for (index, word) in h.iter_mut().enumerate() {
        *word ^= v[index] ^ v[index + 8];
    }
}

#[expect(clippy::many_single_char_names, reason = "RFC 7693's names")]
/// `BLAKE2b`'s mixing function on `v`'s four words at `at`.
fn g(v: &mut [u64; 16], [a, b, c, d]: [usize; 4], x: u64, y: u64) {
    v[a] = v[a].wrapping_add(v[b]).wrapping_add(x);
    v[d] = (v[d] ^ v[a]).rotate_right(32);
    v[c] = v[c].wrapping_add(v[d]);
    v[b] = (v[b] ^ v[c]).rotate_right(24);
    v[a] = v[a].wrapping_add(v[b]).wrapping_add(y);
    v[d] = (v[d] ^ v[a]).rotate_right(16);
    v[c] = v[c].wrapping_add(v[d]);
    v[b] = (v[b] ^ v[c]).rotate_right(63);
}

#[test]
fn blake2b512_matches_rfc_7693() {
    // RFC 7693, appendix A: BLAKE2b-512("abc").
    let expected = "ba80a53f981c4d0d6a2797b69f12f6e94c212f14685ac4b74b12bb6fdbffa2d1\
                    7d87c5392aab792dc252d5de4533cc9518d38aa8dbf1925ab92386edd4009923";
    assert_eq!(super::hex(&blake2b512(b"abc")), expected);
    // The empty message, against its published digest. A multi-block message is checked end to
    // end: minisign-verify accepts what `sign` makes of 200 KiB (`signature/tests.rs`).
    assert_eq!(
        super::hex(&blake2b512(b"")),
        "786a02f742015903c6c6fd852552d272912f4740e15847618a86e217f71f5419\
         d25e1031afee585313896444934eb04b903a685b1448b755d56f701afe9be2ce"
    );
}
