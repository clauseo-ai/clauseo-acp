//! agent-browser's config file: `{}` written at every launch, never through a link.

use super::{CONFIG_DIR, CONFIG_FILE, write_config};
use crate::pipeline::tests::support::TempDir;
use crate::write;

#[test]
fn the_config_file_is_written_as_empty_at_every_launch() {
    let dir = TempDir::new("browser-config");
    let file = write_config(dir.path()).unwrap();
    assert_eq!(file, dir.path().join(CONFIG_DIR).join(CONFIG_FILE));
    assert_eq!(std::fs::read(&file).unwrap(), b"{}\n");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = |path: &std::path::Path| std::fs::metadata(path).unwrap().permissions().mode();
        assert_eq!(mode(&dir.path().join(CONFIG_DIR)) & 0o777, 0o700);
        assert_eq!(mode(&file) & 0o777, 0o600);
    }
    // A planted file is replaced at the next launch: it could choose a cloud browser.
    write::write_file(&file, br#"{"provider":"browserbase"}"#).unwrap();
    write_config(dir.path()).unwrap();
    assert_eq!(std::fs::read(&file).unwrap(), b"{}\n");
}

/// The folder is refused when it is a link; a link at the file's name is replaced, not followed.
#[cfg(unix)]
#[test]
fn no_config_is_written_through_a_link() {
    let dir = TempDir::new("browser-config-link");
    let elsewhere = dir.path().join("elsewhere");
    write::create_dir_all(&elsewhere).unwrap();
    let data = dir.path().join("data");
    write::create_dir_all(&data).unwrap();
    write::symlink(&elsewhere, &data.join(CONFIG_DIR)).unwrap();
    assert!(write_config(&data).is_err());
    assert!(!elsewhere.join(CONFIG_FILE).exists());

    let data = dir.path().join("data2");
    write::create_dir_all(&data.join(CONFIG_DIR)).unwrap();
    let target = elsewhere.join("target.json");
    write::write_file(&target, b"secret").unwrap();
    write::symlink(&target, &data.join(CONFIG_DIR).join(CONFIG_FILE)).unwrap();
    let file = write_config(&data).unwrap();
    assert_eq!(std::fs::read(&target).unwrap(), b"secret");
    assert!(std::fs::symlink_metadata(&file).unwrap().is_file());
    assert_eq!(std::fs::read(&file).unwrap(), b"{}\n");
}
