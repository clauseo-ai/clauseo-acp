//! The relay's one accepted request: an HTTP/1.1 websocket upgrade (RFC 6455 section 4) on
//! exactly `GET /devtools/browser/<secret>`, with `Host: 127.0.0.1:<the relay's port>` and no
//! `Origin`. Everything else, `/json` and `/json/version` included, gets [`NOT_FOUND`] and the
//! connection closes.
//!
//! `Host` must name the relay itself, so a page that rebinds a name to 127.0.0.1 is refused
//! before the secret is even compared; `Origin` is sent by every browser page and never by
//! agent-browser's client (tokio-tungstenite, `cdp/client.rs:205-249`). The secret is compared
//! in constant time. The accept key needs SHA-1 and base64, both written here (SHA-1 is the
//! protocol's fixed choice, not a security property of the relay): the guard takes no crate for
//! twenty lines (`Cargo.toml`, `tests/guard/deps.rs`).

/// The longest request head read: agent-browser's is about 250 bytes.
pub(crate) const MAX_HEAD: usize = 8 * 1024;
/// The reply to anything but the upgrade.
pub(crate) const NOT_FOUND: &[u8] =
    b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
/// The path before the secret.
const PATH: &str = "/devtools/browser/";
/// RFC 6455's key suffix.
const GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
/// The secret's length in bytes: 256 bits.
pub(crate) const SECRET_BYTES: usize = 32;

/// A new secret from the OS's random source, as 64 lower-case hex digits.
pub(crate) fn new_secret() -> std::io::Result<String> {
    let mut bytes = [0_u8; SECRET_BYTES];
    getrandom::fill(&mut bytes).map_err(|error| std::io::Error::other(error.to_string()))?;
    Ok(hex(&bytes))
}

/// Lower-case hex.
fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let digit = |nibble: u8| char::from(DIGITS.get(usize::from(nibble)).copied().unwrap_or(b'0'));
    bytes
        .iter()
        .flat_map(|byte| [digit(byte >> 4), digit(byte & 0x0F)])
        .collect()
}

/// What the relay does with a request head.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// The upgrade: reply with these bytes (the 101), then speak websocket.
    Upgrade(Vec<u8>),
    /// Anything else: [`NOT_FOUND`], then close.
    NotFound,
}

/// Judges `head` (the bytes up to and including the blank line) against the relay's `port` and
/// `secret`. Pure.
pub(crate) fn judge(head: &[u8], port: u16, secret: &str) -> Verdict {
    match accept_key(head, port, secret) {
        Some(accept) => Verdict::Upgrade(
            format!(
                "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
                 Sec-WebSocket-Accept: {accept}\r\n\r\n"
            )
            .into_bytes(),
        ),
        None => Verdict::NotFound,
    }
}

/// The `Sec-WebSocket-Accept` value when `head` is the one accepted upgrade.
fn accept_key(head: &[u8], port: u16, secret: &str) -> Option<String> {
    let text = std::str::from_utf8(head).ok()?;
    let mut lines = text.split("\r\n");
    let mut request = lines.next()?.split(' ');
    let (method, target, version) = (request.next()?, request.next()?, request.next()?);
    if method != "GET" || version != "HTTP/1.1" || request.next().is_some() {
        return None;
    }
    let given = target.strip_prefix(PATH)?;
    if !same(given.as_bytes(), secret.as_bytes()) {
        return None;
    }
    let mut host = None;
    let mut upgrade = false;
    let mut connection = false;
    let mut version_13 = false;
    let mut key = None;
    for line in lines.take_while(|line| !line.is_empty()) {
        let (name, value) = line.split_once(':')?;
        let value = value.trim();
        match name.trim().to_ascii_lowercase().as_str() {
            "host" if host.is_none() => host = Some(value),
            "sec-websocket-key" if key.is_none() => key = Some(value),
            // A second Host or key, or any Origin (every browser page sends one).
            "host" | "origin" | "sec-websocket-key" => return None,
            "upgrade" => upgrade = value.eq_ignore_ascii_case("websocket"),
            "connection" => {
                connection = value
                    .split(',')
                    .any(|token| token.trim().eq_ignore_ascii_case("upgrade"));
            }
            "sec-websocket-version" => version_13 = value == "13",
            _ => {}
        }
    }
    let key = key.filter(|key| key.len() == 24 && key.ends_with("=="))?;
    let ours = format!("127.0.0.1:{port}");
    if host != Some(ours.as_str()) || !upgrade || !connection || !version_13 {
        return None;
    }
    Some(base64(&sha1(format!("{key}{GUID}").as_bytes())))
}

/// Whether `a` and `b` are equal, in time that depends only on their lengths.
fn same(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0_u8, |diff, (x, y)| diff | (x ^ y)) == 0
}

/// The end of a request head in `bytes`: the index just past its blank line.
pub(crate) fn head_end(bytes: &[u8]) -> Option<usize> {
    bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|at| at + 4)
}

/// Standard base64 with padding.
pub(crate) fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut text = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk.first().copied().unwrap_or(0),
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for (i, shift) in [18_u32, 12, 6, 0].into_iter().enumerate() {
            if i <= chunk.len() {
                let index = usize::try_from((n >> shift) & 63).unwrap_or(0);
                text.push(char::from(TABLE.get(index).copied().unwrap_or(b'A')));
            } else {
                text.push('=');
            }
        }
    }
    text
}

/// SHA-1 (FIPS 180-4), for the accept key only.
#[expect(
    clippy::many_single_char_names,
    reason = "FIPS 180-4's own names, a to e, h, w, f, k"
)]
pub(crate) fn sha1(message: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [
        0x6745_2301,
        0xEFCD_AB89,
        0x98BA_DCFE,
        0x1032_5476,
        0xC3D2_E1F0,
    ];
    let bit_len = u64::try_from(message.len()).unwrap_or(0).wrapping_mul(8);
    let mut padded = message.to_vec();
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());
    for block in padded.as_chunks::<64>().0 {
        let mut w = [0_u32; 80];
        for (word, bytes) in w.iter_mut().zip(block.as_chunks::<4>().0) {
            *word = u32::from_be_bytes(*bytes);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = h;
        for (i, word) in w.iter().enumerate() {
            let (f, k) = match i {
                0..20 => ((b & c) | (!b & d), 0x5A82_7999),
                20..40 => (b ^ c ^ d, 0x6ED9_EBA1),
                40..60 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
                _ => (b ^ c ^ d, 0xCA62_C1D6),
            };
            let next = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*word);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = next;
        }
        for (state, add) in h.iter_mut().zip([a, b, c, d, e]) {
            *state = state.wrapping_add(add);
        }
    }
    let mut digest = [0_u8; 20];
    for (out, word) in digest.as_chunks_mut::<4>().0.iter_mut().zip(h) {
        *out = word.to_be_bytes();
    }
    digest
}
