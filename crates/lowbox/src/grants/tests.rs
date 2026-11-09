use std::path::{Path, PathBuf};

use super::*;

mod scratch;

/// A fresh folder for one test under the temp folder's final path, removed when dropped.
struct Root(PathBuf);

impl Root {
    fn new(tag: &str) -> Root {
        let temp = opened(&std::env::temp_dir()).unwrap();
        let base = without_verbatim(&temp).unwrap();
        let root = base.join(format!(
            "clauseo-lowbox-grants-{}-{tag}",
            std::process::id()
        ));
        scratch::remove_all(&root);
        scratch::create_dir_all(&root);
        Root(root)
    }

    /// The folder `rel` below the root, made.
    fn dir(&self, rel: &str) -> PathBuf {
        let path = self.0.join(rel);
        scratch::create_dir_all(&path);
        path
    }

    /// The four browser folders and `browser-pins`, made.
    fn folders(&self) -> (BrowserFolders, PathBuf) {
        let folders = BrowserFolders {
            profile: self.dir("browser/profile"),
            staging: self.dir("browser/staging"),
            tmp: self.dir("browser/tmp"),
            shell: self.dir("shells/157"),
        };
        (folders, self.dir("browser-pins"))
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        scratch::remove_all(&self.0);
    }
}

fn package() -> PackageName {
    PackageName::new("Clauseo.Browser.B01").unwrap()
}

fn refused(folders: &BrowserFolders, pins: &Path) -> bool {
    matches!(grants(&package(), folders, pins), Err(Error::Grant { .. }))
}

#[test]
fn the_four_folders_get_their_rights_for_the_package() {
    let root = Root::new("four");
    let (f, pins) = root.folders();
    let list = grants(&package(), &f, &pins).unwrap();
    let sid = package_sid(&package());
    let got: Vec<(&Path, Rights)> = list.iter().map(|g| (g.path.as_path(), g.rights)).collect();
    assert_eq!(
        got,
        [
            (f.shell.as_path(), Rights::ReadExecute),
            (f.profile.as_path(), Rights::ReadWrite),
            (f.staging.as_path(), Rights::ReadWrite),
            (f.tmp.as_path(), Rights::ReadWrite),
        ]
    );
    assert!(list.iter().all(|g| g.sid == sid));
}

#[test]
fn the_masks_are_icacls_m_and_rx() {
    assert_eq!(Rights::ReadWrite.mask(), 0x0013_01BF);
    assert_eq!(Rights::ReadExecute.mask(), 0x0012_00A9);
}

#[test]
fn a_folder_over_under_or_at_the_pins_is_refused() {
    let root = Root::new("pins");
    let (good, _) = root.folders();
    let pins = root.dir("browser/browser-pins");
    let mut over = good.clone();
    over.profile = root.0.join("browser");
    let mut under = good.clone();
    under.staging = root.dir("browser/browser-pins/x");
    let mut at = good;
    at.tmp = pins.join(".");
    for (f, what) in [(over, "holds"), (under, "inside"), (at, "same")] {
        assert!(refused(&f, &pins), "{what}");
    }
}

#[test]
fn a_sibling_named_like_the_pins_is_allowed() {
    let root = Root::new("sibling");
    let (mut f, _) = root.folders();
    let pins = root.dir("browser/browser-pins");
    f.tmp = root.dir("browser/browser-pins-tmp");
    assert!(grants(&package(), &f, &pins).is_ok());
}

#[test]
fn a_relative_or_missing_folder_or_a_file_is_refused() {
    let root = Root::new("missing");
    let (good, pins) = root.folders();
    let mut relative = good.clone();
    relative.tmp = PathBuf::from("tmp");
    let mut missing = good.clone();
    missing.tmp = root.0.join("browser/none");
    let mut file = good;
    file.tmp = root.0.join("browser/file");
    scratch::write(&file.tmp, b"x");
    for (f, what) in [(relative, "relative"), (missing, "missing"), (file, "file")] {
        assert!(refused(&f, &pins), "{what}");
    }
}

/// `profile\..\browser-pins` would pass a lexical check and resolve to the pins; any `..` is
/// refused, even one that resolves somewhere harmless.
#[test]
fn a_parent_part_is_refused() {
    let root = Root::new("parent");
    let (good, pins) = root.folders();
    let mut to_pins = good.clone();
    to_pins.profile = good.profile.join("..").join("..").join("browser-pins");
    let mut harmless = good.clone();
    harmless.profile = good.profile.join("..").join("profile");
    assert!(refused(&to_pins, &pins));
    assert!(refused(&harmless, &pins));
}

#[test]
fn a_verbatim_nt_device_or_network_path_is_refused() {
    let root = Root::new("prefix");
    let (good, pins) = root.folders();
    for text in [
        r"\\?\C:\ProgramData\Clauseo\tmp",
        r"\??\C:\ProgramData\Clauseo\tmp",
        r"\\.\C:\ProgramData\Clauseo\tmp",
        r"\\localhost\c$\ProgramData\Clauseo\tmp",
        "//?/C:/ProgramData/Clauseo/tmp",
    ] {
        let mut f = good.clone();
        f.tmp = PathBuf::from(text);
        assert!(refused(&f, &pins), "{text}");
    }
    #[cfg(windows)]
    {
        // The real temp folder in its \\?\ form: the same folder, still refused.
        let mut f = good.clone();
        f.tmp = PathBuf::from(format!(r"\\?\{}", good.tmp.display()));
        assert!(refused(&f, &pins));
    }
}

#[test]
fn two_folders_that_are_one_or_nest_are_refused() {
    let root = Root::new("overlap");
    let (good, pins) = root.folders();
    let mut same = good.clone();
    same.tmp = good.profile.clone();
    let mut nested = good.clone();
    nested.staging = root.dir("browser/profile/staging");
    let mut shell_in_profile = good.clone();
    shell_in_profile.shell = root.dir("browser/profile/shell");
    let mut profile_over_shell = good;
    profile_over_shell.profile = root.0.join("shells");
    for (f, what) in [
        (same, "same"),
        (nested, "nested"),
        (shell_in_profile, "shell in profile"),
        (profile_over_shell, "profile over shell"),
    ] {
        assert!(refused(&f, &pins), "{what}");
    }
}

#[cfg(unix)]
#[test]
fn a_folder_through_a_symbolic_link_is_refused() {
    let root = Root::new("symlink");
    let (mut f, pins) = root.folders();
    let link = root.0.join("browser/linked-tmp");
    scratch::symlink(&f.tmp, &link);
    f.tmp = link;
    assert!(refused(&f, &pins));
    // A link to the pins, given as the temp folder: refused too.
    let (mut g, pins) = root.folders();
    let to_pins = root.0.join("browser/pins-link");
    scratch::symlink(&pins, &to_pins);
    g.tmp = to_pins;
    assert!(refused(&g, &pins));
}

#[cfg(windows)]
#[test]
fn a_folder_through_a_junction_is_refused() {
    let root = Root::new("junction");
    let (mut f, pins) = root.folders();
    let link = root.0.join(r"browser\pins-junction");
    scratch::junction(&pins, &link);
    f.tmp = link;
    assert!(refused(&f, &pins));
}

#[cfg(windows)]
#[test]
fn a_folder_by_its_short_name_is_refused() {
    let root = Root::new("short");
    let (mut f, pins) = root.folders();
    let long = root.dir(r"browser\LongTemporaryFolderName");
    let Some(short) = scratch::short_name(&long) else {
        // This volume makes no 8.3 names, so there is no alias to refuse.
        return;
    };
    f.tmp = short;
    assert!(refused(&f, &pins));
}
