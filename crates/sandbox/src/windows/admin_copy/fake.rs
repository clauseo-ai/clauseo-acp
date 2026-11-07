//! A fake [`AdminCopyFs`] for the tests on every OS, and the descriptors they read: a
//! Program Files tree held in memory, each path with what is there and its owner and DACL.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use super::{ADMIN_DIR, AdminCopyFs, FULL_ACCESS, Found, READ_EXECUTE, SETUP_EXE};
use crate::windows::acl_bytes::{self, AceSpec, Acl};
use crate::windows::sids::Sid;

/// `SE_SELF_RELATIVE`.
const SELF_RELATIVE: u16 = 0x8000;
/// `SE_DACL_PRESENT`.
const DACL_PRESENT: u16 = 0x0004;

/// One entry for [`descriptor`]: its type, flags, mask and SID.
pub(crate) type Entry<'a> = (u8, u8, u32, &'a str);

/// A self-relative descriptor with owner `owner` and, when `dacl` is given, that DACL
/// (protected when `protected`); `None` is a NULL DACL.
pub(crate) fn descriptor(owner: &str, dacl: Option<&[Entry<'_>]>, protected: bool) -> Vec<u8> {
    let owner = acl_bytes::sid_bytes(&Sid::parse(owner).unwrap());
    let mut control = SELF_RELATIVE;
    if protected {
        control |= acl_bytes::DACL_PROTECTED;
    }
    let acl = dacl.map(|entries| {
        entries
            .iter()
            .fold(acl_bytes::empty_acl(), |acl, (kind, flags, mask, sid)| {
                let spec = AceSpec {
                    kind: *kind,
                    flags: *flags,
                    mask: *mask,
                    sid: acl_bytes::sid_bytes(&Sid::parse(sid).unwrap()),
                };
                Acl::parse(&acl).unwrap().with(&spec)
            })
    });
    if acl.is_some() || dacl.is_none() {
        control |= DACL_PRESENT;
    }
    let owner_at = 20u32;
    let dacl_at = acl
        .as_ref()
        .map_or(0, |_| owner_at + u32::try_from(owner.len()).unwrap());
    let mut bytes = vec![1, 0];
    bytes.extend(control.to_le_bytes());
    for offset in [owner_at, 0, 0, dacl_at] {
        bytes.extend(offset.to_le_bytes());
    }
    bytes.extend(&owner);
    bytes.extend(acl.unwrap_or_default());
    bytes
}

/// What a stock `C:\Program Files` holds: `TrustedInstaller` owns it and has full control;
/// SYSTEM and Administrators modify it; Users read and run; CREATOR OWNER's full control is
/// inherit-only.
pub(crate) fn program_files_dacl() -> Vec<u8> {
    descriptor(
        super::TRUSTED_INSTALLER,
        Some(&[
            (0, 0, FULL_ACCESS, super::TRUSTED_INSTALLER),
            (0, 0, 0x1_301BF, super::SYSTEM),
            (0, 0, 0x1_301BF, super::ADMINISTRATORS),
            (0, 0, READ_EXECUTE, super::USERS),
            (0, 0x1B, 0x1000_0000, "S-1-3-0"),
        ]),
        false,
    )
}

/// The admin folder or copy as `--adopt` locks it.
pub(crate) fn locked_dacl(folder: bool) -> Vec<u8> {
    let inherit = if folder { 0x3 } else { 0 };
    descriptor(
        super::ADMINISTRATORS,
        Some(&[
            (0, inherit, FULL_ACCESS, super::SYSTEM),
            (0, inherit, FULL_ACCESS, super::ADMINISTRATORS),
            (0, inherit, READ_EXECUTE, super::USERS),
        ]),
        true,
    )
}

/// The recorded hash every [`Fake::ready`] copy has.
pub(crate) const HASH: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

/// The fake machine: paths by their text, each with what is there and its descriptor.
pub(crate) struct Fake {
    pub(crate) program_files: PathBuf,
    pub(crate) paths: BTreeMap<String, (Found, Vec<u8>)>,
    pub(crate) hash: String,
    pub(crate) recorded: Option<String>,
    /// Every path asked about, in order.
    pub(crate) asked: RefCell<Vec<PathBuf>>,
}

impl Fake {
    /// A machine where the admin copy passes every rule.
    pub(crate) fn ready() -> Fake {
        let program_files = PathBuf::from(r"C:\Program Files");
        let mut fake = Fake {
            program_files: program_files.clone(),
            paths: BTreeMap::new(),
            hash: HASH.to_owned(),
            recorded: Some(HASH.to_owned()),
            asked: RefCell::new(Vec::new()),
        };
        fake.set(&program_files, Found::Folder, program_files_dacl());
        let mut at = program_files;
        for part in ADMIN_DIR {
            at = at.join(part);
            fake.set(&at, Found::Folder, locked_dacl(true));
        }
        fake.set(&at.join(SETUP_EXE), Found::File, locked_dacl(false));
        fake
    }

    /// Puts `found` with `security` at `path`.
    pub(crate) fn set(&mut self, path: &Path, found: Found, security: Vec<u8>) {
        self.paths.insert(key(path), (found, security));
    }

    fn get(&self, path: &Path) -> Option<&(Found, Vec<u8>)> {
        self.asked.borrow_mut().push(path.to_path_buf());
        self.paths.get(&key(path))
    }
}

/// A path's text as the fake keys it: `\` throughout, lowercase.
fn key(path: &Path) -> String {
    path.to_string_lossy().replace('/', "\\").to_lowercase()
}

impl AdminCopyFs for Fake {
    fn program_files(&self) -> io::Result<PathBuf> {
        Ok(self.program_files.clone())
    }

    fn found(&self, path: &Path) -> io::Result<Found> {
        Ok(self.get(path).map_or(Found::Missing, |(found, _)| *found))
    }

    fn security(&self, path: &Path) -> io::Result<Vec<u8>> {
        self.get(path)
            .map(|(_, security)| security.clone())
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))
    }

    fn sha256(&self, path: &Path) -> io::Result<String> {
        match self.get(path) {
            Some((Found::File, _)) => Ok(self.hash.clone()),
            _ => Err(io::Error::other("not a regular file")),
        }
    }

    fn recorded_sha256(&self) -> io::Result<Option<String>> {
        Ok(self.recorded.clone())
    }
}
