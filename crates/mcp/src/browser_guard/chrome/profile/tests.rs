//! The kept profile's links: never read through, never written through.

use std::path::{Path, PathBuf};

use super::{named, ready_profile, step};
use crate::write;

/// A fresh `<temp>/clauseo-mcp-profile-<test>-<pid>` with a profile made once in `p`, and an
/// `outside` folder holding `secret.json`.
fn kept(test: &str) -> (PathBuf, PathBuf, PathBuf) {
    let dir =
        std::env::temp_dir().join(format!("clauseo-mcp-profile-{test}-{}", std::process::id()));
    if std::fs::symlink_metadata(&dir).is_ok() {
        write::remove_dir_all(&dir).unwrap();
    }
    let (profile, outside) = (dir.join("p"), dir.join("outside"));
    write::create_dir_all(&outside).unwrap();
    write::write(&outside.join("secret.json"), r#"{"secret": 1}"#).unwrap();
    ready_profile(&profile, &profile.join("s"), &dir.join("d")).unwrap();
    (dir, profile, outside)
}

fn ready_again(dir: &Path, profile: &Path) {
    ready_profile(profile, &profile.join("s"), &dir.join("d")).unwrap();
}

#[cfg(unix)]
fn json(file: &Path) -> serde_json::Value {
    serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap()
}

fn is_plain_file(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|found| found.is_file())
}

#[cfg(unix)]
fn is_real_folder(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|found| found.is_dir())
}

/// A symlink at `Preferences` or `Local State`: its target is neither read nor written, and the
/// link is replaced by the guard's own file.
#[cfg(unix)]
#[test]
fn a_link_at_either_file_is_replaced_never_followed() {
    let (dir, profile, outside) = kept("file-link");
    let secret = outside.join("secret.json");
    for file in [
        profile.join("Default").join("Preferences"),
        profile.join("Local State"),
    ] {
        write::remove_file(&file).unwrap();
        write::symlink(&secret, &file).unwrap();
        ready_again(&dir, &profile);
        assert!(is_plain_file(&file), "{}", file.display());
        assert_eq!(json(&secret), serde_json::json!({"secret": 1}));
        assert!(
            json(&file).get("secret").is_none(),
            "{} read its target",
            file.display()
        );
    }
    assert_eq!(
        json(&profile.join("Default").join("Preferences"))["safebrowsing"]["enabled"],
        false
    );
    write::remove_dir_all(&dir).unwrap();
}

/// A symlink at `Default` or at the staging folder is replaced by a real folder; the folder it
/// pointed at gets nothing.
#[cfg(unix)]
#[test]
fn a_link_at_a_folder_is_replaced_by_a_real_folder() {
    let (dir, profile, outside) = kept("folder-link");
    for folder in [profile.join("Default"), profile.join("s")] {
        write::remove_dir_all(&folder).unwrap();
        write::symlink(&outside, &folder).unwrap();
        ready_again(&dir, &profile);
        assert!(is_real_folder(&folder), "{}", folder.display());
    }
    let mut names: Vec<_> = std::fs::read_dir(&outside)
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name())
        .collect();
    names.sort();
    assert_eq!(names, ["secret.json"]);
    assert!(is_plain_file(&profile.join("Default").join("Preferences")));
    write::remove_dir_all(&dir).unwrap();
}

/// Unix: the write opens its folder once without following a link, so a `Default` that is a
/// link by the time of the write (swapped in after its check) is refused, never written through.
#[cfg(unix)]
#[test]
fn a_folder_swapped_for_a_link_before_the_write_is_refused() {
    let (dir, profile, outside) = kept("swap");
    let default = profile.join("Default");
    write::remove_dir_all(&default).unwrap();
    write::symlink(&outside, &default).unwrap();
    let written = write::replace_beside(&default.join("Preferences"), b"{}");
    assert!(written.is_err(), "wrote through the link");
    let mut names: Vec<_> = std::fs::read_dir(&outside)
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name())
        .collect();
    names.sort();
    assert_eq!(names, ["secret.json"]);
    write::remove_dir_all(&dir).unwrap();
}

/// A temporary file left by an earlier start, even a link, is replaced; none is left behind.
#[cfg(unix)]
#[test]
fn a_left_temporary_file_is_replaced_and_none_stays() {
    let (dir, profile, outside) = kept("temporary");
    let default = profile.join("Default");
    write::symlink(
        &outside.join("secret.json"),
        &default.join(".Preferences.clauseo-new"),
    )
    .unwrap();
    write::write(&profile.join(".Local State.clauseo-new"), "half").unwrap();
    ready_again(&dir, &profile);
    assert!(std::fs::symlink_metadata(default.join(".Preferences.clauseo-new")).is_err());
    assert!(std::fs::symlink_metadata(profile.join(".Local State.clauseo-new")).is_err());
    assert_eq!(
        json(&outside.join("secret.json")),
        serde_json::json!({"secret": 1})
    );
    assert_eq!(
        json(&default.join("Preferences"))["safebrowsing"]["enabled"],
        false
    );
    write::remove_dir_all(&dir).unwrap();
}

/// A write killed midway leaves its temporary file, under any number, and maybe a folder of
/// that name: the next start removes the file, leaves the folder, and still writes.
#[test]
fn leftovers_of_a_killed_write_never_fail_the_next() {
    let (dir, profile, _) = kept("leftovers");
    let default = profile.join("Default");
    write::write(&default.join(".Preferences.clauseo-new-999-0"), "half").unwrap();
    write::write(&default.join(".Preferences.clauseo-new"), "half").unwrap();
    write::create_dir_all(&default.join(".Preferences.clauseo-new-999-1")).unwrap();
    ready_again(&dir, &profile);
    ready_again(&dir, &profile);
    let mut names: Vec<_> = std::fs::read_dir(&default)
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name())
        .collect();
    names.sort();
    assert_eq!(names, [".Preferences.clauseo-new-999-1", "Preferences"]);
    write::remove_dir_all(&dir).unwrap();
}

/// A failed step names itself and the file: "could not write Default/Preferences: ...".
#[cfg(unix)]
#[test]
fn a_failed_write_says_which_file() {
    let (dir, profile, _) = kept("named");
    let file = profile.join("Default").join("Preferences");
    write::remove_file(&file).unwrap();
    write::create_dir_all(&file.join("inside")).unwrap();
    let error = ready_profile(&profile, &profile.join("s"), &dir.join("d")).unwrap_err();
    assert!(
        error
            .to_string()
            .starts_with("could not write Default/Preferences: "),
        "{error}"
    );
    write::remove_dir_all(&dir).unwrap();
}

/// What an error says never holds the folders above the profile (the cwd, maybe the user's home
/// folder): names inside the profile are relative, and the profile itself is "the profile
/// folder", for a swapped folder's error as for any other.
#[test]
fn errors_name_nothing_above_the_profile() {
    let parent = Path::new("/home/alice-secret/work");
    let profile = parent.join("chrome-profile-1-0000abcd");
    assert_eq!(named(&profile, &profile), "the profile folder");
    assert_eq!(named(&profile, &profile.join("Default")), "Default");
    let preferences = profile.join("Default").join("Preferences");
    assert_eq!(named(&profile, &preferences), "Default/Preferences");
    assert_eq!(named(&profile, parent), "a folder outside the profile");
    let swapped = std::io::Error::other("not a real folder");
    let texts = [
        step("make", &profile, &profile.join("Default"), &swapped).to_string(),
        step("make", &profile, &profile, &swapped).to_string(),
        step("write", &profile, &preferences, &swapped).to_string(),
    ];
    assert_eq!(texts[0], "could not make Default: not a real folder");
    assert_eq!(
        texts[1],
        "could not make the profile folder: not a real folder"
    );
    for text in &texts {
        assert!(
            !text.contains("alice-secret") && !text.contains("work"),
            "{text}"
        );
    }
}

/// End to end: a profile whose cwd cannot hold it fails naming "the profile folder", never the
/// cwd; a staging folder that cannot be made fails naming it relative to the profile.
#[cfg(unix)]
#[test]
fn a_failed_start_names_nothing_above_the_profile() {
    let (dir, profile, _) = kept("above");
    let cwd = dir.join("home-of-alice");
    write::write(&cwd, "a file, not a folder").unwrap();
    let error = ready_profile(
        &cwd.join("chrome-profile-1"),
        &cwd.join("x"),
        &dir.join("d"),
    )
    .unwrap_err()
    .to_string();
    assert!(
        error.starts_with("could not make the profile folder: "),
        "{error}"
    );
    assert!(
        !error.contains("home-of-alice") && !error.contains("clauseo-mcp"),
        "{error}"
    );
    // A staging file the read-only profile will not let go of: `real_folder` fails on it.
    let staging = profile.join("s");
    write::remove_dir_all(&staging).unwrap();
    write::write(&staging, "a file").unwrap();
    write::set_mode(&profile, 0o500).unwrap();
    let made = ready_profile(&profile, &staging, &dir.join("d"));
    write::set_mode(&profile, 0o700).unwrap();
    // As root the mode does not hold, and the start goes through: nothing to check then.
    if let Err(error) = made {
        let error = error.to_string();
        assert!(error.starts_with("could not make s: "), "{error}");
        assert!(!error.contains("clauseo-mcp-profile"), "{error}");
    }
    write::remove_dir_all(&dir).unwrap();
}

/// Every OS: a kept profile's files are rewritten in place as plain files, the temporary names
/// gone.
#[test]
fn a_kept_profile_is_rewritten_without_temporary_files() {
    let (dir, profile, _) = kept("plain");
    ready_again(&dir, &profile);
    let mut names: Vec<_> = std::fs::read_dir(profile.join("Default"))
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name())
        .collect();
    names.sort();
    assert_eq!(names, ["Preferences"]);
    assert!(is_plain_file(&profile.join("Local State")));
    write::remove_dir_all(&dir).unwrap();
}
