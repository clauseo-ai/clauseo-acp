use super::{DATA_DIR_VAR, IDENTIFIER, KnownFolder, Locations, Os, SETTINGS_FILE};
use crate::error::Error;
use crate::testing::{FakeDirs, FakeEnv, abs, abs_str};
use std::path::{Path, PathBuf};

fn home_dirs() -> FakeDirs {
    FakeDirs {
        home: Some(PathBuf::from("/home/ada")),
        ..FakeDirs::default()
    }
}

fn windows_dirs() -> FakeDirs {
    windows_dirs_without(None)
}

/// [`windows_dirs`], less the known folder `missing`.
fn windows_dirs_without(missing: Option<KnownFolder>) -> FakeDirs {
    let known = [
        (KnownFolder::LocalAppData, "/C/Users/ada/AppData/Local"),
        (KnownFolder::Documents, "/D/OneDrive/Documents"),
    ];
    FakeDirs {
        home: Some(PathBuf::from("/C/Users/ada")),
        known: known
            .into_iter()
            .filter(|&(folder, _)| Some(folder) != missing)
            .map(|(folder, path)| (folder, PathBuf::from(path)))
            .collect(),
    }
}

#[test]
fn macos_data_dir() {
    let at = Locations::resolve_for(Os::Mac, &home_dirs(), &FakeEnv::default()).unwrap();
    let data = Path::new("/home/ada/Library/Application Support/ai.clauseo.desktop");
    assert_eq!(at.data_dir(), data);
    assert_eq!(at.settings_file(), data.join("settings.json"));
    assert_eq!(
        at.default_matters_dir(),
        Path::new("/home/ada/Documents/Clauseo")
    );
}

#[test]
fn windows_data_dir_is_local_not_roaming() {
    let at = Locations::resolve_for(Os::Windows, &windows_dirs(), &FakeEnv::default()).unwrap();
    assert_eq!(
        at.data_dir(),
        Path::new("/C/Users/ada/AppData/Local/ai.clauseo.desktop")
    );
    // The known folder, wherever it points, not `home/Documents`.
    assert_eq!(
        at.default_matters_dir(),
        Path::new("/D/OneDrive/Documents/Clauseo")
    );
}

#[test]
fn windows_needs_its_known_folders() {
    let no_local = windows_dirs_without(Some(KnownFolder::LocalAppData));
    let error = Locations::resolve_for(Os::Windows, &no_local, &FakeEnv::default()).unwrap_err();
    assert!(matches!(
        error,
        Error::NoFolder {
            folder: "LocalAppData"
        }
    ));
    let no_documents = windows_dirs_without(Some(KnownFolder::Documents));
    let error =
        Locations::resolve_for(Os::Windows, &no_documents, &FakeEnv::default()).unwrap_err();
    assert!(matches!(
        error,
        Error::NoFolder {
            folder: "Documents"
        }
    ));
}

#[test]
fn linux_data_dir_xdg() {
    let env = FakeEnv(vec![
        ("XDG_DATA_HOME", abs_str("xdg/data")),
        ("XDG_DOCUMENTS_DIR", abs_str("xdg/docs")),
    ]);
    let at = Locations::resolve_for(Os::Linux, &home_dirs(), &env).unwrap();
    assert_eq!(at.data_dir(), abs("xdg/data/ai.clauseo.desktop"));
    assert_eq!(at.default_matters_dir(), abs("xdg/docs/Clauseo"));
}

#[test]
fn linux_falls_back_to_home_when_xdg_is_unset_or_relative() {
    for env in [
        FakeEnv::default(),
        FakeEnv(vec![
            ("XDG_DATA_HOME", "data".to_owned()),
            ("XDG_DOCUMENTS_DIR", "docs".to_owned()),
        ]),
    ] {
        let at = Locations::resolve_for(Os::Linux, &home_dirs(), &env).unwrap();
        assert_eq!(
            at.data_dir(),
            Path::new("/home/ada/.local/share/ai.clauseo.desktop")
        );
        assert_eq!(
            at.default_matters_dir(),
            Path::new("/home/ada/Documents/Clauseo")
        );
    }
}

#[test]
fn macos_and_linux_need_a_home() {
    for os in [Os::Mac, Os::Linux] {
        let error =
            Locations::resolve_for(os, &FakeDirs::default(), &FakeEnv::default()).unwrap_err();
        assert!(
            matches!(error, Error::NoFolder { folder: "home" }),
            "{os:?}"
        );
    }
    // XDG folders alone are enough on Linux.
    let env = FakeEnv(vec![
        ("XDG_DATA_HOME", abs_str("d")),
        ("XDG_DOCUMENTS_DIR", abs_str("e")),
    ]);
    assert!(Locations::resolve_for(Os::Linux, &FakeDirs::default(), &env).is_ok());
}

#[test]
fn data_dir_override_from_env() {
    let env = FakeEnv(vec![(DATA_DIR_VAR, abs_str("tmp/clauseo-ci"))]);
    for os in [Os::Windows, Os::Mac, Os::Linux] {
        let at = Locations::resolve_for(os, &windows_dirs(), &env).unwrap();
        assert_eq!(at.data_dir(), abs("tmp/clauseo-ci"), "{os:?}");
        assert_eq!(at.settings_file(), abs("tmp/clauseo-ci/settings.json"));
    }
    // The override moves only the data folder.
    let at = Locations::resolve_for(Os::Windows, &windows_dirs(), &env).unwrap();
    assert_eq!(
        at.default_matters_dir(),
        Path::new("/D/OneDrive/Documents/Clauseo")
    );
}

#[test]
fn data_dir_override_must_be_absolute_and_an_empty_one_is_unset() {
    let relative = FakeEnv(vec![(DATA_DIR_VAR, "relative/dir".to_owned())]);
    let error = Locations::resolve_for(Os::Mac, &home_dirs(), &relative).unwrap_err();
    assert!(matches!(&error, Error::RelativeDataDir { path } if path == Path::new("relative/dir")));
    let empty = FakeEnv(vec![(DATA_DIR_VAR, String::new())]);
    let at = Locations::resolve_for(Os::Mac, &home_dirs(), &empty).unwrap();
    assert!(at.data_dir().ends_with(IDENTIFIER));
}

#[test]
fn under_root_reads_nothing() {
    // `under_root` takes no dirs and no env, so it cannot read either; it only joins paths.
    let at = Locations::under_root(Path::new("/scratch/run-1"));
    assert_eq!(at.data_dir(), Path::new("/scratch/run-1"));
    assert_eq!(
        at.settings_file(),
        Path::new("/scratch/run-1").join(SETTINGS_FILE)
    );
    assert_eq!(
        at.default_matters_dir(),
        Path::new("/scratch/run-1/Documents/Clauseo")
    );
}

#[test]
fn resolve_uses_this_os() {
    let dirs = FakeDirs {
        home: Some(PathBuf::from("/h")),
        known: vec![
            (KnownFolder::LocalAppData, PathBuf::from("/l")),
            (KnownFolder::Documents, PathBuf::from("/d")),
        ],
    };
    let env = FakeEnv::default();
    assert_eq!(
        Locations::resolve(&dirs, &env).unwrap(),
        Locations::resolve_for(Os::CURRENT, &dirs, &env).unwrap()
    );
    let expected = if cfg!(windows) {
        Os::Windows
    } else if cfg!(target_os = "macos") {
        Os::Mac
    } else {
        Os::Linux
    };
    assert_eq!(Os::CURRENT, expected);
}

#[test]
fn the_identifier_is_the_desktop_apps() {
    let config = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../desktop/tauri.conf.json"),
    )
    .unwrap();
    let config: serde_json::Value = serde_json::from_str(&config).unwrap();
    assert_eq!(config["identifier"], IDENTIFIER);
}
