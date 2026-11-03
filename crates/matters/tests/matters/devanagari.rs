//! Devanagari file and folder names through the public paths the host uses (decision 0007,
//! Unicode names): listing, checkpoints, undo, restore and comparing keep every name byte for
//! byte as the file system gave it; the safety matches (Word's owner file, refused and claimed
//! folders) find a name in either Unicode form. Two names that differ only by form are two
//! files on ext4 and NTFS and one on APFS, asserted per OS.
//!
//! Nukta letters U+0958-095F are composition exclusions: NFC decomposes `क़` (U+0958) into `क`
//! and U+093C. U+0929, U+0931 and U+0934 compose: NFC of `र` + U+093C is `ऱ` (U+0931).

use clauseo_matters::{CompareSide, HeldOpen, Paper, RestoreFiles, VersionFile};
use clauseo_protocol::FileChange;
use unicode_normalization::UnicodeNormalization;

use crate::fixture::{
    Fixture, added, assert_no_temp_files, blob, cid, created, deleted, meta, modified, path,
    restore_request, tools, undo,
};
use crate::support;

/// `ऱिपोर्ट.docx` ("report") composed: U+0931.
const REPORT_NFC: &str = "\u{931}\u{93f}\u{92a}\u{94b}\u{930}\u{94d}\u{91f}.docx";
/// The same name decomposed: `र` + nukta U+093C.
const REPORT_NFD: &str = "\u{930}\u{93c}\u{93f}\u{92a}\u{94b}\u{930}\u{94d}\u{91f}.docx";
/// `क़ानून.docx` ("law") with the precomposed nukta letter U+0958, which is neither NFC nor NFD.
const LAW_PRECOMPOSED: &str = "\u{958}\u{93e}\u{928}\u{942}\u{928}.docx";
/// The same name as NFC (and NFD) writes it: `क` + nukta U+093C.
const LAW_DECOMPOSED: &str = "\u{915}\u{93c}\u{93e}\u{928}\u{942}\u{928}.docx";
/// `अनुबंध/किरायानामा.docx`: a lease in a folder named "agreement".
const LEASE: &str = "\u{905}\u{928}\u{941}\u{92c}\u{902}\u{927}/\u{915}\u{93f}\u{930}\u{93e}\u{92f}\u{93e}\u{928}\u{93e}\u{92e}\u{93e}.docx";
/// The lease's folder.
const AGREEMENT: &str = "\u{905}\u{928}\u{941}\u{92c}\u{902}\u{927}";

fn nfc(text: &str) -> String {
    text.nfc().collect()
}

fn nfd(text: &str) -> String {
    text.nfd().collect()
}

/// Whether the test folder's file system finds `b` once `a` exists, asserted per OS: APFS
/// (macOS) looks names up without regard to normalisation; ext4 and NTFS do not.
fn one_file_on_this_os(fx: &Fixture, a: &str, b: &str) -> bool {
    let dir = fx.dir.path().join("probe");
    support::write(dir.join(a), "probe");
    let found = dir.join(b).symlink_metadata().is_ok();
    support::remove_dir_all(&dir);
    assert_eq!(
        found,
        cfg!(target_os = "macos"),
        "{a:?} and {b:?} on {}",
        dir.display()
    );
    found
}

fn paper_paths(papers: &[Paper]) -> Vec<&str> {
    papers.iter().map(|paper| paper.path.as_str()).collect()
}

fn version_paths(files: &[VersionFile]) -> Vec<&str> {
    files.iter().map(|file| file.path.as_str()).collect()
}

/// `changes` sorted by their paths' bytes, so the expected lists need no order of their own.
fn by_path(mut changes: Vec<FileChange>) -> Vec<FileChange> {
    changes.sort_by(|a, b| a.path().as_str().cmp(b.path().as_str()));
    changes
}

/// `names` sorted by their bytes.
fn sorted<'a>(names: &[&'a str]) -> Vec<&'a str> {
    let mut names = names.to_vec();
    names.sort_unstable();
    names
}

#[test]
fn the_test_names_are_in_the_forms_they_claim() {
    assert_eq!(nfc(REPORT_NFD), REPORT_NFC);
    assert_eq!(nfd(REPORT_NFC), REPORT_NFD);
    assert_ne!(REPORT_NFC, REPORT_NFD);
    assert_eq!(nfc(LAW_PRECOMPOSED), LAW_DECOMPOSED);
    assert_eq!(nfd(LAW_PRECOMPOSED), LAW_DECOMPOSED);
    assert_ne!(LAW_PRECOMPOSED, LAW_DECOMPOSED);
    assert_eq!(nfc(LEASE), LEASE);
    assert_eq!(nfd(LEASE), LEASE);
}

/// Papers and checkpoints keep each name in the form it was made in: NFD, a precomposed nukta
/// letter and a Devanagari folder all come back byte for byte.
#[test]
fn papers_and_versions_keep_devanagari_names_byte_for_byte() {
    let fx = Fixture::new("dev-papers");
    let names = [REPORT_NFD, LAW_PRECOMPOSED, LEASE];
    for name in names {
        fx.write(name, name);
    }
    let matter = fx.import();
    let papers = matter.papers(100).unwrap();
    assert_eq!(paper_paths(&papers.papers), sorted(&names));
    assert_eq!(
        version_paths(&matter.files_at(cid(1)).unwrap()),
        sorted(&names)
    );
}

/// A turn changes, deletes and adds Devanagari names; the checkpoint, the comparison, undo and
/// a restore all name each file as it was made, and put back the folder by its own name.
#[test]
fn checkpoint_undo_restore_and_compare_keep_devanagari_names() {
    let fx = Fixture::new("dev-undo");
    fx.write(REPORT_NFD, "v1");
    fx.write(LAW_PRECOMPOSED, "law v1");
    fx.write(LEASE, "lease v1");
    let mut matter = fx.import();
    matter.before_turn(&meta(2)).unwrap();
    fx.write(REPORT_NFD, "v2");
    support::remove_file(fx.file(LAW_PRECOMPOSED));
    support::remove_dir_all(fx.file(AGREEMENT));
    fx.write(REPORT_NFC_SIBLING, "new");
    let step = [REPORT_NFD, LAW_PRECOMPOSED, LEASE, REPORT_NFC_SIBLING];
    let changes = by_path(created(
        matter.after_turn(&meta(3), &tools(&step)).unwrap(),
        3,
    ));
    let expected = by_path(vec![
        modified(REPORT_NFD, b"v1", b"v2"),
        deleted(LAW_PRECOMPOSED, b"law v1"),
        deleted(LEASE, b"lease v1"),
        added(REPORT_NFC_SIBLING, b"new"),
    ]);
    assert_eq!(changes, expected);
    let compared = matter
        .compare(
            CompareSide::Checkpoint(cid(1)),
            CompareSide::Checkpoint(cid(3)),
        )
        .unwrap();
    assert_eq!(by_path(compared), expected);

    let undone = undo(&mut matter, (1, 3), &step, (4, 5)).unwrap();
    assert_eq!(undone.outside_edits, None);
    assert_eq!(fx.read(REPORT_NFD), b"v1");
    assert_eq!(fx.read(LAW_PRECOMPOSED), b"law v1");
    assert_eq!(fx.read(LEASE), b"lease v1");
    assert!(!fx.exists(REPORT_NFC_SIBLING));
    let papers = matter.papers(100).unwrap();
    assert_eq!(
        paper_paths(&papers.papers),
        sorted(&[REPORT_NFD, LAW_PRECOMPOSED, LEASE])
    );

    // Back to checkpoint 3's version, chosen files: the report and the new file come back by
    // their own names; a restore never deletes, so the law and the lease stay.
    let chosen = [path(REPORT_NFD), path(REPORT_NFC_SIBLING)];
    let restored = matter
        .restore(&restore_request(3, RestoreFiles::Only(&chosen), 6, 7))
        .unwrap();
    assert_eq!(
        by_path(created(restored.checkpoint, 7)),
        by_path(vec![
            modified(REPORT_NFD, b"v1", b"v2"),
            added(REPORT_NFC_SIBLING, b"new"),
        ])
    );
    assert_eq!(fx.read(REPORT_NFD), b"v2");
    assert_eq!(fx.read(REPORT_NFC_SIBLING), b"new");
    assert_eq!(
        version_paths(&matter.files_at(cid(7)).unwrap()),
        sorted(&[REPORT_NFD, LAW_PRECOMPOSED, LEASE, REPORT_NFC_SIBLING])
    );
    assert_no_temp_files(&fx.folder);
}

/// `ऱिपोर्ट नई.docx` ("new report"), NFC: a second name for the turn to add.
const REPORT_NFC_SIBLING: &str =
    "\u{931}\u{93f}\u{92a}\u{94b}\u{930}\u{94d}\u{91f} \u{928}\u{908}.docx";

/// Two names that differ only by Unicode form: two files on ext4 and NTFS, each listed and
/// versioned by its own bytes; one file on APFS, listed by the name it was made with.
#[test]
fn names_differing_only_by_form_are_two_files_except_on_apfs() {
    let fx = Fixture::new("dev-forms");
    let one = one_file_on_this_os(&fx, REPORT_NFC, REPORT_NFD)
        & one_file_on_this_os(&fx, LAW_PRECOMPOSED, LAW_DECOMPOSED);
    fx.write(REPORT_NFC, "composed");
    fx.write(REPORT_NFD, "decomposed");
    fx.write(LAW_PRECOMPOSED, "precomposed");
    fx.write(LAW_DECOMPOSED, "decomposed nukta");
    let matter = fx.import();
    let listed = matter.papers(100).unwrap();
    let files = matter.files_at(cid(1)).unwrap();
    if one {
        // APFS kept the first spelling and the second write replaced its content.
        let names = sorted(&[REPORT_NFC, LAW_PRECOMPOSED]);
        assert_eq!(paper_paths(&listed.papers), names);
        assert_eq!(version_paths(&files), names);
        assert_eq!(fx.read(REPORT_NFC), b"decomposed");
    } else {
        let names = sorted(&[REPORT_NFC, REPORT_NFD, LAW_PRECOMPOSED, LAW_DECOMPOSED]);
        assert_eq!(paper_paths(&listed.papers), names);
        assert_eq!(version_paths(&files), names);
        let report = files.iter().find(|file| file.path.as_str() == REPORT_NFD);
        assert_eq!(report.map(|file| &file.content), Some(&blob(b"decomposed")));
    }
}

/// Word's owner file is found whichever Unicode form it and the document are in: an NFD owner
/// file beside an NFC document and the other way round, and a nukta letter precomposed on one
/// side and decomposed on the other. On ext4 and NTFS the exact stats miss these.
#[test]
fn an_owner_file_in_another_form_holds_the_document() {
    // (document, the owner file beside it). The lease's owner file drops two characters, as
    // Word does for a long stem, in a Devanagari folder.
    let lease_name = LEASE.rsplit('/').next().unwrap();
    let cases = [
        (REPORT_NFC, format!("~${REPORT_NFD}")),
        (REPORT_NFD, format!("~${REPORT_NFC}")),
        (LAW_PRECOMPOSED, format!("~${LAW_DECOMPOSED}")),
        (LAW_DECOMPOSED, format!("~${LAW_PRECOMPOSED}")),
        (
            LEASE,
            format!("~${}", lease_name.chars().skip(2).collect::<String>()),
        ),
    ];
    for (n, (document, owner)) in cases.iter().enumerate() {
        let fx = Fixture::new(&format!("dev-owner-{n}"));
        fx.write(document, "doc");
        let dir = document.rsplit_once('/').map_or("", |(dir, _)| dir);
        let owner_rel = if dir.is_empty() {
            owner.clone()
        } else {
            format!("{dir}/{owner}")
        };
        fx.write(&owner_rel, "owner");
        let held = clauseo_matters::held_open(&fx.folder, &path(document));
        assert!(
            matches!(held, Some(HeldOpen::OwnerFile(_))),
            "{document:?} beside {owner:?}: {held:?}"
        );
        // The name comes back as the file system gives it, which APFS may spell in either form.
        let at = clauseo_matters::held_open_at(&fx.folder, &fx.file(document));
        assert_eq!(
            at.map(|(found, _)| nfc(found.as_str())),
            Some(nfc(document)),
            "{document:?}"
        );
    }
}

/// Undo refuses with `FileLocked` when Word holds the document by an owner file in the other
/// form, and changes nothing.
#[test]
fn undo_refuses_a_document_held_by_an_owner_file_in_another_form() {
    let fx = Fixture::new("dev-owner-undo");
    fx.write(REPORT_NFC, "one");
    let mut matter = fx.import();
    matter.before_turn(&meta(2)).unwrap();
    fx.write(REPORT_NFC, "two");
    created(
        matter.after_turn(&meta(3), &tools(&[REPORT_NFC])).unwrap(),
        3,
    );
    fx.write(&format!("~${REPORT_NFD}"), "owner file: open in Word");
    let err = undo(&mut matter, (1, 3), &[REPORT_NFC], (4, 5)).unwrap_err();
    assert!(
        matches!(&err, clauseo_matters::Error::FileLocked { paths } if *paths == [path(REPORT_NFC)]),
        "{err:?}"
    );
    assert_eq!(fx.read(REPORT_NFC), b"two");
    assert_eq!(matter.last_checkpoint(), Some(cid(3)));
}

/// "Already a matter" and refused folders compare folder names composed: a folder named in NFD
/// is the same as, or inside, a matter or refused folder named in NFC, on every OS.
#[test]
fn a_folder_in_another_form_is_the_claimed_or_refused_folder() {
    let fx = Fixture::new("dev-folders");
    let composed = fx.dir.path().join(REPORT_NFC.trim_end_matches(".docx"));
    let decomposed = fx.dir.path().join(REPORT_NFD.trim_end_matches(".docx"));
    support::create_dir_all(composed.join("sub"));
    if !one_file_on_this_os(&fx, REPORT_NFC, REPORT_NFD) {
        support::create_dir_all(&decomposed);
    }
    assert!(clauseo_matters::same_or_nested(&decomposed, &composed));
    assert!(clauseo_matters::same_or_nested(
        &composed.join("sub"),
        &decomposed
    ));
    let err = clauseo_matters::check_place(&decomposed, &[composed.join("sub")]).unwrap_err();
    assert!(
        matches!(err, clauseo_matters::Error::FolderRefused { .. }),
        "{err:?}"
    );
    let other = fx.dir.path().join(AGREEMENT);
    support::create_dir_all(&other);
    assert!(!clauseo_matters::same_or_nested(&other, &composed));
}

/// A matter folder and a sibling whose name differs only by Unicode form. Counting a path as
/// the matter's allows (a tool call's path joins the turn's), so [`matter_path_in`] matches as
/// the file system finds names: on ext4 and NTFS the sibling's file lies outside. Checking for
/// Word's owner file refuses, so [`held_open_at`] folds to NFC: a write through the sibling's
/// spelling to a held document is refused on every OS.
///
/// [`matter_path_in`]: clauseo_matters::matter_path_in
/// [`held_open_at`]: clauseo_matters::held_open_at
#[test]
fn a_sibling_folder_in_another_form_is_outside_but_its_held_name_is_refused() {
    let fx = Fixture::new("dev-sibling");
    let matter = fx.dir.path().join(REPORT_NFC.trim_end_matches(".docx"));
    let sibling = fx.dir.path().join(REPORT_NFD.trim_end_matches(".docx"));
    let apfs = one_file_on_this_os(&fx, REPORT_NFC, REPORT_NFD);
    for name in ["Contract.docx", "~$ntract.docx"] {
        support::write(matter.join(name), "x");
    }
    if !apfs {
        support::write(sibling.join("Contract.docx"), "the sibling's own");
    }
    let inside = clauseo_matters::matter_path_in(&matter, &matter.join("Contract.docx"));
    assert_eq!(inside, Some(path("Contract.docx")));
    let through_sibling = clauseo_matters::matter_path_in(&matter, &sibling.join("Contract.docx"));
    assert_eq!(through_sibling, apfs.then(|| path("Contract.docx")));
    let held = clauseo_matters::held_open_at(&matter, &sibling.join("Contract.docx"));
    assert_eq!(
        held,
        Some((
            path("Contract.docx"),
            HeldOpen::OwnerFile("~$ntract.docx".to_owned())
        ))
    );
}
