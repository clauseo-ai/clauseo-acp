//! The package name, SIDs, and the package SID derived from the name.
//!
//! The derivation is `DeriveAppContainerSidFromAppContainerName`'s, reimplemented so it runs on
//! every OS and needs no userenv call: SHA-256 of the name lowercased, in UTF-16LE; its first 28
//! bytes are seven little-endian sub-authorities after `S-1-15-2`. A test on Windows checks it
//! against the API. The package name is ASCII only, so lowercasing it has one answer.

mod sha256;

use std::fmt;

use crate::error::Error;

/// The longest package name, as `CreateAppContainerProfile` allows.
const MAX_NAME: usize = 64;
/// The most sub-authorities a SID holds (`SID_MAX_SUB_AUTHORITIES`).
const MAX_SUB_AUTHORITIES: usize = 15;
/// The largest identifier authority: six bytes.
const MAX_AUTHORITY: u64 = (1 << 48) - 1;
/// `SECURITY_APP_PACKAGE_AUTHORITY`.
const APP_PACKAGE_AUTHORITY: u64 = 15;
/// `SECURITY_APP_PACKAGE_BASE_RID`.
const APP_PACKAGE_BASE_RID: u32 = 2;

/// An `AppContainer` package name, such as `Clauseo.Browser.B01`: 1 to 64 ASCII letters, digits,
/// dots and dashes. Kept lowercased: Windows derives the SID from the lowercased name, so two
/// names that differ only in letter case are one package, and compare and hash as one.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PackageName(String);

impl PackageName {
    /// The name, checked and lowercased.
    pub fn new(name: &str) -> Result<PackageName, Error> {
        if name.is_empty() {
            return Err(Error::PackageName { reason: "empty" });
        }
        if name.len() > MAX_NAME {
            return Err(Error::PackageName {
                reason: "longer than 64 characters",
            });
        }
        if !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
        {
            return Err(Error::PackageName {
                reason: "not only ASCII letters, digits, dots and dashes",
            });
        }
        Ok(PackageName(name.to_ascii_lowercase()))
    }

    /// The name, lowercased.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A SID. Its `Debug` hides the value: a package SID is never logged, and a user's SID has no
/// place in a log either. [`Sid::as_str`] is the one way to the text.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Sid {
    /// The identifier authority (5 for NT, 15 for app packages).
    authority: u64,
    /// The sub-authorities, at most 15.
    subs: Vec<u32>,
    /// The `S-1-…` text.
    text: String,
}

impl Sid {
    /// A SID from its `S-1-<authority>-<sub>…` text (decimal parts, 0 to 15 sub-authorities).
    pub fn parse(text: &str) -> Result<Sid, Error> {
        let mut parts = text.split('-');
        if parts.next() != Some("S") || parts.next() != Some("1") {
            return Err(Error::SidText);
        }
        let authority = parts
            .next()
            .and_then(decimal::<u64>)
            .filter(|a| *a <= MAX_AUTHORITY)
            .ok_or(Error::SidText)?;
        let subs = parts
            .map(|p| decimal::<u32>(p).ok_or(Error::SidText))
            .collect::<Result<Vec<u32>, Error>>()?;
        Sid::from_parts(authority, subs)
    }

    /// A SID from its authority and sub-authorities.
    fn from_parts(authority: u64, subs: Vec<u32>) -> Result<Sid, Error> {
        if subs.len() > MAX_SUB_AUTHORITIES || authority > MAX_AUTHORITY {
            return Err(Error::SidText);
        }
        Ok(Sid::checked(authority, subs))
    }

    /// A SID from parts already within the limits.
    fn checked(authority: u64, subs: Vec<u32>) -> Sid {
        let mut text = format!("S-1-{authority}");
        for sub in &subs {
            text.push('-');
            text.push_str(&sub.to_string());
        }
        Sid {
            authority,
            subs,
            text,
        }
    }

    /// The `S-1-…` text. Never log a package SID's.
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// The binary SID as Windows lays it out: revision 1, the sub-authority count, the authority
    /// in six big-endian bytes, then each sub-authority in four little-endian bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(8 + 4 * self.subs.len());
        bytes.push(1);
        // At most 15, checked when made.
        bytes.push(u8::try_from(self.subs.len()).unwrap_or(u8::MAX));
        bytes.extend_from_slice(&self.authority.to_be_bytes()[2..]);
        for sub in &self.subs {
            bytes.extend_from_slice(&sub.to_le_bytes());
        }
        bytes
    }

    /// [`Sid::to_bytes`] in a buffer of `u32`s, so a pointer to it has the 4-byte alignment
    /// the kernel's SID capture probes for. A binary SID is 8 + 4 × n bytes, so it fills whole
    /// words; each word holds four of its bytes in memory order.
    pub(crate) fn to_aligned(&self) -> Vec<u32> {
        let bytes = self.to_bytes();
        let (words, _none_left) = bytes.as_chunks::<4>();
        words.iter().map(|w| u32::from_ne_bytes(*w)).collect()
    }
}

impl fmt::Debug for Sid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Sid(..)")
    }
}

/// A decimal number with no sign, no leading `+` and at least one digit.
fn decimal<T: std::str::FromStr>(part: &str) -> Option<T> {
    if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    part.parse().ok()
}

/// The package SID of `name`, `S-1-15-2-…`, as `DeriveAppContainerSidFromAppContainerName`
/// derives it. No profile needs to be registered. Never log it.
pub fn package_sid(name: &PackageName) -> Sid {
    // Lowercased already (`PackageName::new`).
    let utf16: Vec<u8> = name
        .as_str()
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    let hash = sha256::digest(&utf16);
    let mut subs = vec![APP_PACKAGE_BASE_RID];
    let (words, _none_left) = hash.as_chunks::<4>();
    subs.extend(words.iter().take(7).map(|w| u32::from_le_bytes(*w)));
    // 8 sub-authorities and authority 15: within the limits.
    Sid::checked(APP_PACKAGE_AUTHORITY, subs)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// The W2 (j) recipe's vector: `Get-NtSid -PackageName 'Clauseo.Browser.B01'` on the VM.
    const B01: &str = "S-1-15-2-4291742471-3177126929-1079639061-2360262239-4224205492-\
                       3771709147-1509459664";

    /// A name at the 64-character limit.
    pub(crate) const LONGEST: &str =
        "Clauseo.Browser.Longest-Name-At-The-Limit-0123456789-ABCDEFGHIJK";
    /// Its package SID, from Python: `hashlib.sha256(name.lower().encode("utf-16-le"))`.
    const LONGEST_SID: &str = "S-1-15-2-2091307814-3276343069-1066385404-277365165-\
                               1844948808-800365280-310130186";

    #[test]
    fn the_package_sid_matches_the_vm_vector() {
        let name = PackageName::new("Clauseo.Browser.B01").unwrap();
        assert_eq!(package_sid(&name).as_str(), B01);
    }

    #[test]
    fn the_derivation_ignores_letter_case() {
        let upper = PackageName::new("CLAUSEO.BROWSER.B01").unwrap();
        assert_eq!(package_sid(&upper).as_str(), B01);
    }

    #[test]
    fn names_that_differ_in_case_are_one_package() {
        use std::collections::HashSet;
        let mixed = PackageName::new("Clauseo.Browser.B01").unwrap();
        let lower = PackageName::new("clauseo.browser.b01").unwrap();
        assert_eq!(mixed, lower);
        assert_eq!(mixed.as_str(), "clauseo.browser.b01");
        let set: HashSet<PackageName> = [mixed, lower].into_iter().collect();
        assert_eq!(set.len(), 1);
    }

    /// A 64-character name hashes three SHA-256 blocks (128 bytes of UTF-16 and the padding);
    /// the value is Python's `hashlib.sha256` of the lowercased name in UTF-16LE, and
    /// `windows::tests` checks the same name against `DeriveAppContainerSidFromAppContainerName`.
    #[test]
    fn the_longest_name_matches_an_independent_sha256() {
        let name = PackageName::new(LONGEST).unwrap();
        assert_eq!(package_sid(&name).as_str(), LONGEST_SID);
    }

    #[test]
    fn the_aligned_sid_holds_the_same_bytes() {
        let sid = Sid::parse(B01).unwrap();
        let words = sid.to_aligned();
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_ne_bytes()).collect();
        assert_eq!(bytes, sid.to_bytes());
    }

    #[test]
    fn a_package_sid_round_trips_through_its_text() {
        let name = PackageName::new("Clauseo.Browser.B01").unwrap();
        let sid = package_sid(&name);
        assert_eq!(Sid::parse(sid.as_str()).unwrap(), sid);
    }

    #[test]
    fn package_names_are_checked() {
        assert!(PackageName::new("").is_err());
        assert!(PackageName::new(&"a".repeat(65)).is_err());
        assert!(PackageName::new(&"a".repeat(64)).is_ok());
        assert!(PackageName::new("Clauseo Browser").is_err());
        assert!(PackageName::new("Clauseo\\Browser").is_err());
        assert!(PackageName::new("Clauséo").is_err());
        assert!(PackageName::new("Clauseo.Browser-B_01").is_err());
        assert!(PackageName::new("Clauseo.Browser-B01").is_ok());
    }

    #[test]
    fn sid_text_is_checked() {
        assert!(Sid::parse("S-1-5-18").is_ok());
        assert!(Sid::parse("S-1-16").is_ok());
        for bad in [
            "",
            "S-1",
            "S-2-5-18",
            "s-1-5-18",
            "S-1-5-",
            "S-1--5",
            "S-1-5-+18",
            "S-1-5-4294967296",
            "S-1-281474976710656",
            "S-1-5-1-2-3-4-5-6-7-8-9-10-11-12-13-14-15-16",
        ] {
            assert_eq!(Sid::parse(bad), Err(Error::SidText), "{bad}");
        }
    }

    #[test]
    fn the_binary_sid_has_the_windows_layout() {
        assert_eq!(
            Sid::parse("S-1-5-18").unwrap().to_bytes(),
            [1, 1, 0, 0, 0, 0, 0, 5, 18, 0, 0, 0]
        );
        let bytes = Sid::parse(B01).unwrap().to_bytes();
        assert_eq!(bytes.len(), 8 + 4 * 8);
        assert_eq!(&bytes[..12], &[1, 8, 0, 0, 0, 0, 0, 15, 2, 0, 0, 0]);
        assert_eq!(&bytes[12..16], &4_291_742_471_u32.to_le_bytes());
    }

    #[test]
    fn debug_hides_the_sid() {
        let sid = Sid::parse(B01).unwrap();
        assert_eq!(format!("{sid:?}"), "Sid(..)");
    }
}
