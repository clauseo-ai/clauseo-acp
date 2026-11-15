//! `reveal` of a file the browser saved in Clauseo's own downloads folder for a matter: by the
//! matter and the name, recomputed under the app data folder, never a path from the page.

use super::*;

/// The app data folder under `scratch` with `judgment.pdf` in the matter's fallback folder.
fn saved(scratch: &Scratch, matter: MatterId) -> (PathBuf, PathBuf) {
    let app_data = scratch.path().join("data");
    let folder = clauseo_host::matter_fallback_downloads(&app_data, matter);
    let rel = folder.strip_prefix(scratch.path()).unwrap().to_owned();
    scratch.dir(rel.to_str().unwrap());
    let file = scratch.file(rel.join("judgment.pdf").to_str().unwrap(), b"%PDF");
    (app_data, file)
}

fn download(matter: MatterId, name: &str) -> RevealTarget {
    RevealTarget::Download {
        matter,
        name: name.to_owned(),
    }
}

#[test]
fn reveal_shows_a_saved_file_selected_in_clauseos_folder_for_the_matter() {
    let scratch = Scratch::new("reveal-download", &[]);
    let (folder, brief, _) = matter_folder(&scratch);
    let host = host(&folder, &brief);
    let (app_data, file) = saved(&scratch, host.located.matter);
    let opener = Arc::new(FakeOpener::default());
    let target = download(host.located.matter, "judgment.pdf");
    block_on(reveal(&host, &opener, &app_data, FOLDER_WAIT, target)).unwrap();
    assert_eq!(opener.opened(), [Opened::Reveal(file)]);
}

#[test]
fn reveal_refuses_a_name_with_a_separator_or_dots_a_missing_file_and_an_unknown_matter() {
    let scratch = Scratch::new("reveal-download-refused", &[]);
    let (folder, brief, _) = matter_folder(&scratch);
    let host = host(&folder, &brief);
    let matter = host.located.matter;
    let (app_data, _) = saved(&scratch, matter);
    scratch.file("data/secret.txt", b"x");
    let opener = Arc::new(FakeOpener::default());
    let reveal_of = |target| block_on(reveal(&host, &opener, &app_data, FOLDER_WAIT, target));
    for name in [
        "",
        ".",
        "..",
        "../notes",
        "../../../secret.txt",
        "sub/judgment.pdf",
        "sub\\judgment.pdf",
        "/etc/passwd",
        "gone.pdf",
    ] {
        let refused = reveal_of(download(matter, name));
        assert_eq!(code(refused), ErrorCode::CommandInvalid, "{name:?}");
    }
    let unknown = reveal_of(download(matter_id(), "judgment.pdf"));
    assert_eq!(code(unknown), ErrorCode::CommandUnknownMatter);
    assert_eq!(opener.opened(), []);
}

#[cfg(unix)]
#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "test set-up makes a link; the desktop crate has no write module"
)]
fn reveal_refuses_a_link_in_the_folder_and_a_linked_folder() {
    let scratch = Scratch::new("reveal-download-link", &[]);
    let (folder, brief, _) = matter_folder(&scratch);
    let host = host(&folder, &brief);
    let matter = host.located.matter;
    let (app_data, file) = saved(&scratch, matter);
    let link = file.with_file_name("link.pdf");
    std::os::unix::fs::symlink(&brief, &link).unwrap();
    let opener = Arc::new(FakeOpener::default());
    let refused = block_on(reveal(
        &host,
        &opener,
        &app_data,
        FOLDER_WAIT,
        download(matter, "link.pdf"),
    ));
    assert_eq!(code(refused), ErrorCode::CommandInvalid);
    // The downloads folder itself a link to a folder holding the name.
    let other = MatterId::new(uuid::Uuid::now_v7()).unwrap();
    let host = FakeHost {
        located: LocatedFile {
            matter: other,
            ..host.located.clone()
        },
        ..host
    };
    let linked = clauseo_host::matter_fallback_downloads(&app_data, other);
    scratch.dir(
        linked
            .parent()
            .unwrap()
            .strip_prefix(scratch.path())
            .unwrap()
            .to_str()
            .unwrap(),
    );
    std::os::unix::fs::symlink(file.parent().unwrap(), &linked).unwrap();
    let refused = block_on(reveal(
        &host,
        &opener,
        &app_data,
        FOLDER_WAIT,
        download(other, "judgment.pdf"),
    ));
    assert_eq!(code(refused), ErrorCode::CommandInvalid);
    assert_eq!(opener.opened(), []);
}

#[test]
fn a_download_target_reads_a_matter_and_a_name() {
    let matter = matter_id();
    let read: RevealTarget = serde_json::from_value(serde_json::json!({
        "download": { "matter": matter.to_string(), "name": "judgment.pdf" }
    }))
    .unwrap();
    assert_eq!(read, download(matter, "judgment.pdf"));
    for bad in [
        serde_json::json!({ "download": { "matter": matter.to_string() } }),
        serde_json::json!({ "download": { "path": "/etc/passwd" } }),
        serde_json::json!({ "download": "judgment.pdf" }),
    ] {
        assert!(
            serde_json::from_value::<RevealTarget>(bad.clone()).is_err(),
            "{bad}"
        );
    }
}
