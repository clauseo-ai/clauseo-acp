//! The parsed list and its matching: every host in one string, an end offset per entry, a tag
//! bit per entry, and an open-addressing hash index of entry numbers. Four allocations however
//! long the list, so loading and dropping a million entries is cheap and a lookup is one probe
//! per label.

use std::borrow::Cow;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use crate::Generated;

/// Why a host is listed. Serialises as `malware` or `phishing`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Tag {
    /// `m`: malware (today, known botnet command-and-control addresses).
    Malware,
    /// `p`: phishing.
    Phishing,
}

impl Tag {
    /// Reads the list's letter.
    pub(crate) fn from_letter(letter: &[u8]) -> Option<Tag> {
        match letter {
            b"m" => Some(Tag::Malware),
            b"p" => Some(Tag::Phishing),
            _ => None,
        }
    }

    /// The word for logs and the `Clauseo-Blocked` header: `malware` or `phishing`.
    pub fn word(self) -> &'static str {
        match self {
            Tag::Malware => "malware",
            Tag::Phishing => "phishing",
        }
    }
}

/// A listed host that matched: the entry, and its tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hit<'a> {
    /// The entry as stored: a domain, or an IP address in brackets (`[1.2.3.4]`, `[2001:db8::1]`).
    pub listed: &'a str,
    /// Why it is listed.
    pub tag: Tag,
}

/// A parsed, verified list, or the empty list (nothing loaded). Read-only once built.
#[derive(Clone, PartialEq, Eq)]
pub struct BlockList {
    generated: Option<Generated>,
    /// Every entry, back to back: domains as listed, IP literals as `[address]` in Rust's
    /// canonical form.
    text: Box<str>,
    /// Where each entry ends in `text`.
    ends: Box<[u32]>,
    /// One bit per entry: set for phishing, clear for malware.
    phishing: Box<[u64]>,
    /// The hash index: 0 is empty, else the entry's number plus one. A power of two long.
    slots: Box<[u32]>,
}

impl std::fmt::Debug for BlockList {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BlockList")
            .field("generated", &self.generated)
            .field("entries", &self.len())
            .finish_non_exhaustive()
    }
}

impl Default for BlockList {
    fn default() -> BlockList {
        BlockList::empty()
    }
}

impl BlockList {
    /// No list loaded: matches nothing, and has no `generated`.
    pub fn empty() -> BlockList {
        BlockList {
            generated: None,
            text: Box::default(),
            ends: Box::default(),
            phishing: Box::default(),
            slots: Box::default(),
        }
    }

    /// When the list was made; `None` for the empty list.
    pub fn generated(&self) -> Option<Generated> {
        self.generated
    }

    /// True when no list is loaded ([`BlockList::empty`]); a loaded list of no entries is not.
    pub fn is_unloaded(&self) -> bool {
        self.generated.is_none()
    }

    /// The number of entries.
    pub fn len(&self) -> usize {
        self.ends.len()
    }

    /// True when the list has no entries.
    pub fn is_empty(&self) -> bool {
        self.ends.is_empty()
    }

    /// The bytes the list holds on the heap.
    pub fn heap_bytes(&self) -> usize {
        self.text.len() + self.ends.len() * 4 + self.phishing.len() * 8 + self.slots.len() * 4
    }

    /// Matches a request host: case folded, one trailing dot dropped, an IP literal (dotted
    /// IPv4, IPv6 bare or in brackets) matched exactly, a domain matched when it or any parent
    /// on a label boundary is listed. IDNs must already be ASCII (`xn--`), as the proxy gives
    /// them.
    pub fn lookup(&self, host: &str) -> Option<Hit<'_>> {
        let host = host.strip_suffix('.').unwrap_or(host);
        if host.is_empty() || self.is_empty() {
            return None;
        }
        let bare = host.strip_prefix('[').and_then(|h| h.strip_suffix(']'));
        if let Some(Ok(ip)) = bare.map(str::parse::<std::net::Ipv6Addr>) {
            return self.lookup_ip(IpAddr::V6(ip));
        }
        if let Ok(ip) = host.parse::<IpAddr>() {
            return self.lookup_ip(ip);
        }
        let host = if host.bytes().any(|b| b.is_ascii_uppercase()) {
            Cow::Owned(host.to_ascii_lowercase())
        } else {
            Cow::Borrowed(host)
        };
        let mut rest: &str = &host;
        loop {
            if let Some(hit) = self.find(rest) {
                return Some(hit);
            }
            rest = rest.split_once('.')?.1;
        }
    }

    /// Matches an IP address exactly. An IPv6 address that carries an IPv4 address (mapped,
    /// translated, compatible, NAT64 `64:ff9b::/96` or 6to4 `2002::/16`, the forms the proxy's
    /// address check reads the same way) also matches that IPv4 address's entry.
    pub fn lookup_ip(&self, ip: IpAddr) -> Option<Hit<'_>> {
        if self.is_empty() {
            return None;
        }
        let ip = ip.to_canonical();
        if let Some(hit) = self.find(&format!("[{ip}]")) {
            return Some(hit);
        }
        match ip {
            IpAddr::V6(v6) => self.find(&format!("[{}]", embedded_v4(v6)?)),
            IpAddr::V4(_) => None,
        }
    }

    /// The entry equal to `key`, as stored.
    fn find(&self, key: &str) -> Option<Hit<'_>> {
        let mask = self.slots.len().checked_sub(1)?;
        let mut at = hash(key) & mask;
        loop {
            let slot = *self.slots.get(at)?;
            let entry = usize::try_from(slot.checked_sub(1)?).ok()?;
            let listed = self.entry(entry)?;
            if listed == key {
                return Some(Hit {
                    listed,
                    tag: self.tag(entry),
                });
            }
            at = (at + 1) & mask;
        }
    }

    fn entry(&self, entry: usize) -> Option<&str> {
        let start = match entry.checked_sub(1) {
            Some(before) => *self.ends.get(before)?,
            None => 0,
        };
        let end = *self.ends.get(entry)?;
        self.text.get(start as usize..end as usize)
    }

    fn tag(&self, entry: usize) -> Tag {
        let word = self.phishing.get(entry / 64).copied().unwrap_or(0);
        if word >> (entry % 64) & 1 == 1 {
            Tag::Phishing
        } else {
            Tag::Malware
        }
    }
}

/// The IPv4 address an IPv6 address carries, in the forms `address.rs`'s `v6_public` reads.
fn embedded_v4(ip: Ipv6Addr) -> Option<Ipv4Addr> {
    let [s0, s1, s2, s3, s4, s5, s6, s7] = ip.segments();
    let v4 = |high: u16, low: u16| Ipv4Addr::from((u32::from(high) << 16) | u32::from(low));
    let zero = |segments: &[u16]| segments.iter().all(|&s| s == 0);
    let mapped = zero(&[s0, s1, s2, s3, s4]) && s5 == 0xffff;
    let translated = zero(&[s0, s1, s2, s3]) && s4 == 0xffff && s5 == 0;
    let compatible = zero(&[s0, s1, s2, s3, s4, s5]);
    let nat64 = s0 == 0x64 && s1 == 0xff9b && zero(&[s2, s3, s4, s5]);
    if mapped || translated || compatible || nat64 {
        return Some(v4(s6, s7));
    }
    (s0 == 0x2002).then(|| v4(s1, s2))
}

/// FNV-1a, 64 bits: short keys, and the table's contents are the signed list's, so no one
/// outside can choose collisions among them.
fn hash(key: &str) -> usize {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in key.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    // The table never passes 2^32 slots, so the low 32 bits are all the mask uses.
    usize::try_from(hash & u64::from(u32::MAX)).unwrap_or_default()
}

/// Builds a [`BlockList`] entry by entry, in the list's order.
#[derive(Default)]
pub(crate) struct Builder {
    text: String,
    ends: Vec<u32>,
    phishing: Vec<u64>,
}

impl Builder {
    pub(crate) fn with_capacity(entries: usize) -> Builder {
        Builder {
            text: String::with_capacity(entries.saturating_mul(20)),
            ends: Vec::with_capacity(entries),
            phishing: Vec::with_capacity(entries.div_ceil(64)),
        }
    }

    /// Adds an entry already in its stored form. False when the text would pass `u32`.
    pub(crate) fn push(&mut self, stored: &str, tag: Tag) -> bool {
        let entry = self.ends.len();
        self.text.push_str(stored);
        let Ok(end) = u32::try_from(self.text.len()) else {
            return false;
        };
        self.ends.push(end);
        if entry.is_multiple_of(64) {
            self.phishing.push(0);
        }
        if tag == Tag::Phishing
            && let Some(word) = self.phishing.last_mut()
        {
            *word |= 1 << (entry % 64);
        }
        true
    }

    /// The list, with its hash index at a load of at most three quarters. An entry whose stored
    /// form repeats an earlier one's (two spellings of one IPv6 address) is left out of the
    /// index; the first wins.
    pub(crate) fn finish(self, generated: Generated) -> BlockList {
        let mut list = BlockList {
            generated: Some(generated),
            text: self.text.into_boxed_str(),
            ends: self.ends.into_boxed_slice(),
            phishing: self.phishing.into_boxed_slice(),
            slots: Box::default(),
        };
        let size = (list.len().saturating_mul(4) / 3 + 1)
            .next_power_of_two()
            .max(8);
        let mut slots = vec![0u32; size];
        let mask = size - 1;
        for entry in 0..list.len() {
            let Some(key) = list.entry(entry) else {
                continue;
            };
            let mut at = hash(key) & mask;
            while let Some(&slot) = slots.get(at) {
                if slot == 0 {
                    break;
                }
                let taken = usize::try_from(slot - 1).ok().and_then(|e| list.entry(e));
                if taken == Some(key) {
                    at = usize::MAX;
                    break;
                }
                at = (at + 1) & mask;
            }
            if let (Some(slot), Ok(number)) = (slots.get_mut(at), u32::try_from(entry + 1)) {
                *slot = number;
            }
        }
        list.slots = slots.into_boxed_slice();
        list
    }
}
