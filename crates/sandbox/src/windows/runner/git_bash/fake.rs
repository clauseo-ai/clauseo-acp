//! The fake machine [`GitBashOs`] for the tests on every OS: a tree that passes every check,
//! a machine stub that passes its own, and the outer runner's own stub's folder, each effect
//! recorded.

use std::collections::HashMap;

use super::*;
use crate::windows::admin_copy::fake::descriptor;
use crate::windows::admin_copy::{FULL_ACCESS, READ_EXECUTE};

pub(super) const ADMIN: &str = r"C:\Program Files\Clauseo\Sandbox";
pub(super) const POOL: &str = "S-1-5-21-1-2-3-1005";
pub(super) const GROUP: &str = "S-1-5-21-1-2-3-1000";
pub(super) const LAWYER: &str = "S-1-5-21-1-2-3-1001";
pub(super) const ID: &str = "2.47.1.windows.1-0123456789ab";

pub(super) fn sid(text: &str) -> Sid {
    Sid::parse(text).unwrap()
}

pub(super) fn tree() -> String {
    format!(r"{ADMIN}\git\ClauseoSb05\{ID}")
}

pub(super) fn image(id: &[u8]) -> Vec<u8> {
    let mut image = b"MZ\x90\0 some code ".to_vec();
    image.extend_from_slice(id);
    image.extend_from_slice(b"\0 more");
    image
}

pub(super) const APP_DATA: &str = r"C:\Users\Lawyer\AppData\Local";
pub(super) const RUNNER_HASH: &str = "ab12";
pub(super) const SYSTEM: &str = "S-1-5-18";
pub(super) const ADMINS: &str = "S-1-5-32-544";

/// The machine stub's descriptor as set-up leaves it: owner SYSTEM, SYSTEM and Administrators
/// full, the group read and execute.
pub(super) fn safe_stub() -> Vec<u8> {
    descriptor(
        SYSTEM,
        Some(&[
            (0, 0x10, FULL_ACCESS, SYSTEM),
            (0, 0x10, FULL_ACCESS, ADMINS),
            (0, 0x10, READ_EXECUTE, GROUP),
        ]),
        false,
    )
}

/// A machine whose tree passes every check, each effect recorded.
pub(super) struct Fake {
    pub(super) record: Result<Option<String>, String>,
    pub(super) admin: Result<WinPath, String>,
    pub(super) levels: Result<(), String>,
    pub(super) marker: Result<String, String>,
    pub(super) runtime: Result<Runtime, String>,
    pub(super) session: u32,
    pub(super) existing: Vec<String>,
    pub(super) fail_make: bool,
    pub(super) fail_hold: bool,
    pub(super) checked: Vec<(String, String)>,
    pub(super) made: HashMap<String, String>,
    pub(super) stub: Result<Vec<u8>, String>,
    pub(super) stub_levels: Vec<(String, String)>,
    pub(super) app_data: Result<String, String>,
    pub(super) secure: Result<(), String>,
    pub(super) secured: Vec<(String, String)>,
    pub(super) runner: Result<String, String>,
    pub(super) files: HashMap<String, String>,
    pub(super) copy: Result<String, String>,
    pub(super) copies: usize,
}

impl Fake {
    pub(super) fn new() -> Fake {
        Fake {
            record: Ok(Some(tree())),
            admin: Ok(WinPath::parse(ADMIN).unwrap()),
            levels: Ok(()),
            marker: Ok(format!("{ID}\n{POOL}\n")),
            runtime: Ok(Runtime {
                final_path: format!(r"\\?\{}\usr\bin\msys-2.0.dll", tree()),
                image: image(b"%%% MSYS shared id: msys-2.0S5"),
            }),
            session: 2,
            existing: Vec::new(),
            fail_make: false,
            fail_hold: false,
            checked: Vec::new(),
            made: HashMap::new(),
            stub: Ok(safe_stub()),
            stub_levels: Vec::new(),
            app_data: Ok(APP_DATA.to_owned()),
            secure: Ok(()),
            secured: Vec::new(),
            runner: Ok(RUNNER_HASH.to_owned()),
            files: HashMap::new(),
            copy: Ok(RUNNER_HASH.to_owned()),
            copies: 0,
        }
    }

    pub(super) fn run(&mut self) -> Result<Ready<String>, Off> {
        prepare(self, &sid(POOL), "ClauseoSb05", &sid(LAWYER))
    }
}

impl GitBashOs for Fake {
    type Held = String;

    fn record(&mut self, pool_user: &Sid) -> Result<Option<String>, String> {
        assert_eq!(pool_user.as_str(), POOL);
        self.record.clone()
    }

    fn admin_dir(&mut self) -> Result<WinPath, String> {
        self.admin.clone()
    }

    fn group(&mut self) -> Result<Sid, String> {
        Ok(sid(GROUP))
    }

    fn check_levels(&mut self, admin: &WinPath, levels: &[(String, String)]) -> Result<(), String> {
        assert_eq!(admin.display(), ADMIN);
        self.checked = levels.to_vec();
        self.levels.clone()
    }

    fn marker(&mut self, path: &WinPath) -> Result<String, String> {
        assert!(
            path.display()
                .eq_ignore_ascii_case(&format!(r"{}\.clauseo-tree", tree()))
        );
        self.marker.clone()
    }

    fn runtime(&mut self, dll: &WinPath) -> Result<Runtime, String> {
        assert!(
            dll.display()
                .eq_ignore_ascii_case(&format!(r"{}\usr\bin\msys-2.0.dll", tree()))
        );
        self.runtime.clone()
    }

    fn hold(&mut self, file: &WinPath) -> Result<String, String> {
        assert!(file.within(&WinPath::parse(&tree()).unwrap()));
        if self.fail_hold {
            return Err("holding bash.exe failed (32)".to_owned());
        }
        Ok(file.display())
    }

    fn session(&mut self) -> Result<u32, String> {
        Ok(self.session)
    }

    fn make_dir(&mut self, path: &str, sddl: &str) -> Result<String, Made> {
        if self.existing.iter().any(|there| there == path) {
            return Err(Made::Exists);
        }
        if self.fail_make {
            return Err(Made::Failed(
                "NtCreateDirectoryObject failed (0xC0000022)".to_owned(),
            ));
        }
        self.made.insert(path.to_owned(), sddl.to_owned());
        Ok(path.to_owned())
    }

    fn stub_security(
        &mut self,
        admin: &WinPath,
        levels: &[(String, String)],
        name: &str,
    ) -> Result<Vec<u8>, String> {
        assert_eq!(admin.display(), ADMIN);
        assert_eq!(name, "bash.exe");
        self.stub_levels = levels.to_vec();
        self.stub.clone()
    }

    fn local_app_data(&mut self) -> Result<String, String> {
        self.app_data.clone()
    }

    fn secure_dir(&mut self, dir: &WinPath, sddl: &str) -> Result<(), String> {
        self.secured.push((dir.display(), sddl.to_owned()));
        self.secure.clone()
    }

    fn runner_hash(&mut self) -> Result<String, String> {
        self.runner.clone()
    }

    fn file_hash(&mut self, file: &WinPath) -> Result<Option<String>, String> {
        Ok(self.files.get(&file.display()).cloned())
    }

    fn copy_runner(&mut self, to: &WinPath) -> Result<(), String> {
        self.copies += 1;
        let hash = self.copy.clone()?;
        self.files.insert(to.display(), hash);
        Ok(())
    }
}
