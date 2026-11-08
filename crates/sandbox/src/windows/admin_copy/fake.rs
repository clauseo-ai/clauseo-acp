//! A fake [`AdminCopyFs`] for the tests on every OS, and the descriptors they read: a
//! Program Files tree held in memory, each path with what is there and its owner and DACL.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use super::{ADMIN_DIR, AdminCopyFs, FULL_ACCESS, Found, Held, READ_EXECUTE, SETUP_EXE};
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

/// What a stock `C:\` holds: `TrustedInstaller` owns it; SYSTEM and Administrators have full
/// control and Users read and run, each inherited; Authenticated Users may add a folder there,
/// and their modify right is inherit-only.
pub(crate) fn volume_root_dacl() -> Vec<u8> {
    descriptor(
        super::TRUSTED_INSTALLER,
        Some(&[
            (0, 0x3, FULL_ACCESS, super::SYSTEM),
            (0, 0x3, FULL_ACCESS, super::ADMINISTRATORS),
            (0, 0x3, READ_EXECUTE, super::USERS),
            (0, 0xB, 0x1_301BF, AUTHENTICATED_USERS),
            (0, 0, super::APPEND_DATA, AUTHENTICATED_USERS),
        ]),
        false,
    )
}

/// `NT AUTHORITY\Authenticated Users`.
pub(crate) const AUTHENTICATED_USERS: &str = "S-1-5-11";

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

/// The admin folder or copy as `--adopt` and the set-up lock it once the record names
/// `group`: [`locked_dacl`]'s entries after a deny of everything for `group`.
pub(crate) fn locked_dacl_denying(folder: bool, group: &str) -> Vec<u8> {
    let inherit = if folder { 0x3 } else { 0 };
    descriptor(
        super::ADMINISTRATORS,
        Some(&[
            (1, inherit, FULL_ACCESS, group),
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
    /// Whether the record names another pool owner.
    pub(crate) another: bool,
    /// Whether the record's owners can't be read.
    pub(crate) owner_unreadable: bool,
    /// The record's `GroupSid`, and whether it can't be read.
    pub(crate) group: Option<Sid>,
    pub(crate) group_unreadable: bool,
    /// This user's `SecurityBlockedAt` mark: whether it is there.
    pub(crate) security_mark: std::cell::Cell<bool>,
    /// Every path asked about, in order.
    pub(crate) asked: RefCell<Vec<PathBuf>>,
    /// What a launch did, in order: `held` when the copy is hashed through a held handle,
    /// `released` when that handle closes (the tests add what they start).
    pub(crate) events: Rc<RefCell<Vec<&'static str>>>,
}

/// A held handle: says `released` when it drops.
struct Release(Rc<RefCell<Vec<&'static str>>>);

impl Drop for Release {
    fn drop(&mut self) {
        self.0.borrow_mut().push("released");
    }
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
            another: false,
            owner_unreadable: false,
            group: None,
            group_unreadable: false,
            security_mark: std::cell::Cell::new(false),
            asked: RefCell::new(Vec::new()),
            events: Rc::new(RefCell::new(Vec::new())),
        };
        fake.set(Path::new(r"C:\"), Found::Folder, volume_root_dacl());
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

    fn sha256_held(&self, path: &Path) -> io::Result<(String, Held)> {
        let hash = self.sha256(path)?;
        self.events.borrow_mut().push("held");
        Ok((hash, Held::new(Release(Rc::clone(&self.events)))))
    }

    fn recorded_sha256(&self) -> io::Result<Option<String>> {
        Ok(self.recorded.clone())
    }

    fn recorded_group(&self) -> io::Result<Option<Sid>> {
        if self.group_unreadable {
            return Err(io::Error::other("the record can't be read"));
        }
        Ok(self.group.clone())
    }

    fn another_owner(&self) -> io::Result<bool> {
        if self.owner_unreadable {
            return Err(io::Error::other("the record can't be read"));
        }
        Ok(self.another)
    }

    fn security_blocked(&self) -> io::Result<bool> {
        Ok(self.security_mark.get())
    }

    fn set_security_blocked(&self, blocked: bool) -> io::Result<()> {
        self.security_mark.set(blocked);
        Ok(())
    }

    fn entries(&self, dir: &Path) -> io::Result<Vec<String>> {
        let prefix = format!("{}\\", key(dir));
        Ok(self
            .paths
            .keys()
            .filter_map(|path| path.strip_prefix(&prefix))
            .filter(|rest| !rest.contains('\\'))
            .map(str::to_owned)
            .collect())
    }
}
