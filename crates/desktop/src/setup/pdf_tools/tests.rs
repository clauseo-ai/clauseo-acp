use std::path::{Path, PathBuf};

use super::{bundled_folder, usable};
use crate::test_support::Scratch;

#[test]
fn on_macos_the_folder_is_under_contents_helpers() {
    let macos = PathBuf::from("/Applications/Clauseo.app/Contents/MacOS");
    let want = PathBuf::from("/Applications/Clauseo.app/Contents/Helpers/pdf");
    let resources = PathBuf::from("/Applications/Clauseo.app/Contents/Resources");
    assert_eq!(
        bundled_folder(true, &macos, Some(&resources)),
        Some(want.clone())
    );
    assert_eq!(bundled_folder(true, &macos, None), Some(want));
    assert_eq!(bundled_folder(true, Path::new("/"), None), None);
}

#[test]
fn elsewhere_the_folder_is_under_the_resource_folder() {
    let install = std::env::temp_dir()
        .join("AppData")
        .join("Local")
        .join("Clauseo");
    assert_eq!(
        bundled_folder(false, &install, Some(&install)),
        Some(install.join("pdf"))
    );
    assert_eq!(bundled_folder(false, &install, None), None);
}

#[test]
fn the_folder_is_usable_only_with_all_three_programs() {
    let suffix = std::env::consts::EXE_SUFFIX;
    let scratch = Scratch::new("pdf-tools-usable", &["pdf"]);
    let folder = scratch.dir("pdf");
    assert!(!usable(&folder));
    scratch.file(&format!("pdf/pdftoppm{suffix}"), b"program");
    // On Windows a program is an .exe file; elsewhere it needs its execute bit.
    #[cfg(unix)]
    scratch.executable(&format!("pdf/pdftoppm{suffix}"));
    assert!(!usable(&folder));
    scratch.file(&format!("pdf/pdfinfo{suffix}"), b"program");
    #[cfg(unix)]
    scratch.executable(&format!("pdf/pdfinfo{suffix}"));
    assert!(!usable(&folder));
    scratch.file(&format!("pdf/pdftotext{suffix}"), b"program");
    #[cfg(unix)]
    scratch.executable(&format!("pdf/pdftotext{suffix}"));
    assert!(usable(&folder));
    assert!(!usable(&scratch.path().join("missing")));
}
