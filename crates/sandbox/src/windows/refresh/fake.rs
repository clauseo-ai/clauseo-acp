//! The refresh tests' fake machine, whose files are byte vectors, and their shared fixtures.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use super::fake_watch::FakeWatch;
use super::test_signer::{TestKey, comment, image};
use super::*;

pub(super) const OWN: &str = "1.4.0";
pub(super) const ADMIN: &str = r"C:\Program Files\Clauseo\Sandbox\clauseo-sandbox-setup.exe";
pub(super) const INSTALL: &str = r"C:\Users\Rohan\AppData\Local\Clauseo";
pub(super) const PROFILE: &str = r"C:\Users\Rohan";
pub(super) const OWNER: &str = "S-1-5-21-1-2-3-1001";

/// The steps a crash can stop the run at, in the order they run.
pub(super) const STEPS: [&str; 7] = [
    "stage",
    "secure_staged",
    "hold",
    "set_aside",
    "place_staged",
    "write_version",
    "write_sha256",
];

/// The fake machine: each file as bytes, or `None` when absent.
pub(super) struct Fake {
    pub(super) system: bool,
    pub(super) exe: PathBuf,
    pub(super) record: RefreshRecord,
    pub(super) profile: Option<PathBuf>,
    /// What `admin_only_dir` says of the install folder.
    pub(super) admin_only: bool,
    pub(super) source_exe: Result<Vec<u8>, SourceError>,
    pub(super) source_sig: Result<Vec<u8>, SourceError>,
    /// What `stage` reads back, when it differs from what it was given.
    pub(super) staged_reads_back: Option<Vec<u8>>,
    pub(super) admin_file: Option<Vec<u8>>,
    pub(super) staged: Option<Vec<u8>>,
    pub(super) set_aside: Option<Vec<u8>>,
    /// The step that fails, as a failed call.
    pub(super) fail: Option<&'static str>,
    /// The step the machine stops at: it and everything after do nothing.
    pub(super) crash: Option<&'static str>,
    pub(super) crashed: bool,
    pub(super) calls: Vec<&'static str>,
    pub(super) logs: Vec<(bool, String)>,
    /// What `reapply_wsl` gives.
    pub(super) wsl: Result<Option<String>, Failed>,
    /// What each `reapply_wsl` gives in turn, before `wsl` once it is empty.
    pub(super) wsl_passes: Vec<Result<Option<String>, Failed>>,
    /// The waits `pause` was asked for.
    pub(super) paused: Vec<std::time::Duration>,
    /// The clock, `AppID` keys, notification and installer `--wsl-deny`'s schedule sees.
    pub(super) watch: FakeWatch,
    /// What `upgrade_wsl` gives.
    pub(super) upgrade: WslUpgraded,
    /// What `pool_busy` gives.
    pub(super) pool_busy: Result<bool, Failed>,
    /// The staging folder's state.
    pub(super) staging: Staging,
}

/// The fake's staging folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Staging {
    /// Not there.
    Absent,
    /// There.
    There,
    /// There, with the set-aside copy, both scheduled for deletion at restart.
    AtRestart,
}

impl Fake {
    /// A machine where the admin copy is `old`, recorded, and the user folder holds `new`
    /// signed by `key` with `sig_comment`.
    pub(super) fn new(old: &[u8], new: &[u8], key: &TestKey, sig_comment: &str) -> Fake {
        Fake {
            system: true,
            exe: PathBuf::from(ADMIN),
            record: RefreshRecord {
                copy_owner: Some(OWNER.to_owned()),
                user_install_dir: Some(INSTALL.to_owned()),
                admin_copy_sha256: Some(sha_hex(old)),
                admin_copy_version: Some(OLD_VERSION.to_owned()),
            },
            profile: Some(PathBuf::from(PROFILE)),
            admin_only: false,
            source_exe: Ok(new.to_vec()),
            source_sig: Ok(key.sign(new, sig_comment)),
            staged_reads_back: None,
            admin_file: Some(old.to_vec()),
            staged: None,
            set_aside: None,
            fail: None,
            crash: None,
            crashed: false,
            calls: Vec::new(),
            logs: Vec::new(),
            staging: Staging::Absent,
            wsl: Ok(None),
            wsl_passes: Vec::new(),
            paused: Vec::new(),
            watch: FakeWatch::default(),
            upgrade: WslUpgraded::NotPending,
            pool_busy: Ok(false),
        }
    }

    /// Records the call; `Err` when it fails or the machine has crashed (doing nothing).
    pub(super) fn step(&mut self, name: &'static str) -> Result<(), Failed> {
        self.calls.push(name);
        if self.crash == Some(name) {
            self.crashed = true;
        }
        if self.crashed {
            return Err(format!("{name}: the machine is down"));
        }
        if self.fail == Some(name) {
            return Err(format!("{name} failed"));
        }
        Ok(())
    }

    /// Whether the admin folder is held now: a hold, the last call either way, went through (a
    /// failed one stops the run before any rename).
    pub(super) fn holding(&self) -> bool {
        let last = self
            .calls
            .iter()
            .rev()
            .find(|call| ["hold", "release"].contains(call));
        last == Some(&"hold")
    }

    /// The invariant: the record never names a hash the admin path doesn't hold, unless it is
    /// the hash it held before the run (the old copy's, which a crash between the renames
    /// leaves with no file).
    pub(super) fn record_names_what_is_there(&self, old: &[u8]) -> bool {
        match (&self.record.admin_copy_sha256, &self.admin_file) {
            (Some(hash), Some(file)) => *hash == sha_hex(file) || *hash == sha_hex(old),
            (Some(hash), None) => *hash == sha_hex(old),
            (None, _) => true,
        }
    }
}

impl Refresher for Fake {
    fn is_system(&mut self) -> Result<bool, Failed> {
        self.calls.push("is_system");
        Ok(self.system)
    }
    fn current_exe(&mut self) -> Result<PathBuf, Failed> {
        Ok(self.exe.clone())
    }
    fn admin_copy(&mut self) -> Result<PathBuf, Failed> {
        Ok(PathBuf::from(ADMIN))
    }
    fn lock(&mut self) -> Result<bool, Failed> {
        // `fail: Some("lock")` is another holder.
        self.calls.push("lock");
        Ok(self.fail != Some("lock"))
    }
    fn lock_waiting(&mut self) -> Result<bool, Failed> {
        // `fail: Some("lock_waiting")` is another holder past the wait.
        self.calls.push("lock_waiting");
        Ok(self.fail != Some("lock_waiting"))
    }
    fn unlock(&mut self) {
        self.calls.push("unlock");
    }
    fn record(&mut self) -> Result<RefreshRecord, Failed> {
        self.calls.push("record");
        Ok(self.record.clone())
    }
    fn clear_leftovers(&mut self) -> Result<(), Failed> {
        self.step("clear_leftovers")?;
        self.set_aside = None;
        self.staged = None;
        Ok(())
    }
    fn profile_dir(&mut self, owner: &Sid) -> Result<Option<PathBuf>, Failed> {
        assert_eq!(owner.as_str(), OWNER, "the record's owner");
        Ok(self.profile.clone())
    }
    fn admin_only_dir(&mut self, dir: &Path) -> Result<bool, Failed> {
        assert_eq!(dir, Path::new(INSTALL), "the record's folder");
        self.calls.push("admin_only_dir");
        Ok(self.admin_only)
    }
    fn read_sources(&mut self, dir: &Path) -> Result<Sources, (SourceFile, SourceError)> {
        assert_eq!(dir, Path::new(INSTALL), "the record's folder, nothing else");
        self.calls.push("read_source");
        let exe = self.source_exe.clone().map_err(|e| (SourceFile::Exe, e))?;
        let sig = self.source_sig.clone().map_err(|e| (SourceFile::Sig, e))?;
        Ok(Sources { exe, sig })
    }
    fn stage(&mut self, exe: &[u8]) -> Result<Vec<u8>, Failed> {
        self.step("stage")?;
        self.staging = Staging::There;
        let staged = self
            .staged_reads_back
            .clone()
            .unwrap_or_else(|| exe.to_vec());
        self.staged = Some(staged.clone());
        Ok(staged)
    }
    fn admin_copy_sha256(&mut self) -> Result<Option<[u8; 32]>, Failed> {
        Ok(self
            .admin_file
            .as_ref()
            .map(|file| Sha256::digest(file).into()))
    }
    fn secure_staged(&mut self) -> Result<(), Failed> {
        self.step("secure_staged")
    }
    fn hold_admin_folder(&mut self) -> Result<(), Failed> {
        self.step("hold")
    }
    fn release_admin_folder(&mut self) {
        self.calls.push("release");
    }
    fn set_aside_admin_copy(&mut self) -> Result<(), Failed> {
        assert!(self.holding(), "a rename only while the folder is held");
        self.step("set_aside")?;
        self.set_aside = self.admin_file.take();
        Ok(())
    }
    fn place_staged(&mut self) -> Result<(), Failed> {
        assert!(self.holding(), "a rename only while the folder is held");
        self.step("place_staged")?;
        assert!(self.admin_file.is_none(), "a move never replaces a file");
        self.admin_file = self.staged.take();
        Ok(())
    }
    fn restore_admin_copy(&mut self) -> Result<(), Failed> {
        assert!(self.holding(), "a rename only while the folder is held");
        self.step("restore")?;
        self.admin_file = self.set_aside.take();
        Ok(())
    }
    fn write_version(&mut self, version: &str) -> Result<(), Failed> {
        self.step("write_version")?;
        self.record.admin_copy_version = Some(version.to_owned());
        Ok(())
    }
    fn write_sha256(&mut self, hex: &str) -> Result<(), Failed> {
        self.step("write_sha256")?;
        self.record.admin_copy_sha256 = Some(hex.to_owned());
        Ok(())
    }
    fn discard_staged(&mut self) {
        self.calls.push("discard_staged");
        if !self.crashed {
            self.staged = None;
        }
    }
    fn leave_staging(&mut self, swapped: bool) {
        self.calls.push("leave_staging");
        if self.crashed {
            return;
        }
        if swapped {
            self.staging = Staging::AtRestart;
        } else if self.staged.is_none() && self.set_aside.is_none() {
            // Removing a folder that still holds a file fails; a restored copy left it empty.
            self.staging = Staging::Absent;
        }
    }
    fn log(&mut self, error: bool, line: &str) {
        self.logs.push((error, line.to_owned()));
    }
    fn reapply_wsl(&mut self) -> Result<Option<String>, Failed> {
        self.calls.push("reapply_wsl");
        if let Some(outcome) = self.watch.reapply() {
            outcome
        } else if self.wsl_passes.is_empty() {
            self.wsl.clone()
        } else {
            self.wsl_passes.remove(0)
        }
    }
    fn pause(&mut self, time: std::time::Duration) {
        self.calls.push("pause");
        self.paused.push(time);
        self.watch.advance(self.watch.clock.saturating_add(time));
    }
}

impl WslUpgrade for Fake {
    fn pool_busy(&mut self) -> Result<bool, Failed> {
        self.calls.push("pool_busy");
        self.pool_busy.clone()
    }

    fn upgrade_wsl(&mut self) -> WslUpgraded {
        self.calls.push("upgrade_wsl");
        self.upgrade.clone()
    }
}

pub(super) fn sha_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

/// The old copy's version, as the record holds it.
pub(super) const OLD_VERSION: &str = "1.3.0.0";

pub(super) fn old() -> Vec<u8> {
    image([1, 3, 0, 0], b"the old setup exe")
}

pub(super) fn new() -> Vec<u8> {
    image([1, 4, 0, 0], b"the new setup exe, signed")
}

pub(super) fn key() -> TestKey {
    TestKey::new(0x5EED_0000_0000_0001)
}

pub(super) fn good() -> String {
    comment(signature::SIGNED_FILE, OWN)
}

/// The trusted comment for `exe` at its own `FILEVERSION`'s `X.Y.Z` (`OWN` when it has none).
pub(super) fn good_for(exe: &[u8]) -> String {
    let version = pe_version::file_version(exe)
        .map_or_else(|| OWN.to_owned(), |[x, y, z, _]| format!("{x}.{y}.{z}"));
    comment(signature::SIGNED_FILE, &version)
}

/// [`refresh`] on `fake`, trusting `key` and the x64 CPU type.
pub(super) fn run_fake(fake: &mut Fake, key: &TestKey) -> RefreshExit {
    run_on(fake, &key.public(), pe_version::MACHINE_X64)
}

/// [`refresh`] on `fake`, trusting `key` (Tauri's form) and `machine`.
pub(super) fn run_on(fake: &mut Fake, key: &str, machine: u16) -> RefreshExit {
    refresh(fake, &Trust { key, machine })
}
