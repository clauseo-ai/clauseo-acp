use clauseo_protocol::{MatterPath, NoticeKind, NoticeLevel, SkippedFile, SkippedFileReason};

use super::{changed_notice, gone_notice};

fn gone(path: &str, reason: SkippedFileReason) -> SkippedFile {
    SkippedFile {
        path: MatterPath::new(path).unwrap(),
        reason,
    }
}

#[test]
fn no_gone_file_is_no_notice() {
    assert_eq!(gone_notice(&[]), None);
}

#[test]
fn one_gone_file_is_named_in_the_text_and_the_kind() {
    let file = gone("Drafts/Scan.pdf", SkippedFileReason::CloudOnly);
    let notice = gone_notice(std::slice::from_ref(&file)).unwrap();
    assert_eq!(notice.level, NoticeLevel::Warning);
    assert_eq!(
        notice.text,
        "Drafts/Scan.pdf was deleted during this answer. Clauseo had no copy of it to bring back."
    );
    assert_eq!(
        notice.kind,
        Some(NoticeKind::FilesGoneNoCopy {
            files: vec![file],
            more: 0,
        })
    );
}

#[test]
fn past_five_gone_files_the_rest_are_counted() {
    let files: Vec<SkippedFile> = (0..7)
        .map(|n| gone(&format!("f{n}.mov"), SkippedFileReason::OverSizeCap))
        .collect();
    let notice = gone_notice(&files).unwrap();
    assert!(
        notice
            .text
            .starts_with("7 files were deleted during this answer")
    );
    assert_eq!(
        notice.kind,
        Some(NoticeKind::FilesGoneNoCopy {
            files: files[..5].to_vec(),
            more: 2,
        })
    );
}

#[test]
fn a_file_changed_with_no_earlier_copy_is_told_and_the_rest_counted() {
    assert_eq!(changed_notice(&[]), None);
    let file = gone("Contract.docx", SkippedFileReason::CloudOnly);
    let notice = changed_notice(std::slice::from_ref(&file)).unwrap();
    assert_eq!(notice.level, NoticeLevel::Warning);
    assert_eq!(
        notice.text,
        "Contract.docx was changed during this answer. Clauseo had no copy of how it was before, \
         so Undo can't bring the earlier version back."
    );
    assert_eq!(
        notice.kind,
        Some(NoticeKind::FilesChangedNoCopy {
            files: vec![file],
            more: 0,
        })
    );
    let files: Vec<SkippedFile> = (0..7)
        .map(|n| gone(&format!("f{n}.mov"), SkippedFileReason::OverSizeCap))
        .collect();
    let notice = changed_notice(&files).unwrap();
    assert!(
        notice
            .text
            .starts_with("7 files Clauseo had no earlier copy of were changed")
    );
    assert_eq!(
        notice.kind,
        Some(NoticeKind::FilesChangedNoCopy {
            files: files[..5].to_vec(),
            more: 2,
        })
    );
    // Six: all six named, never one left to count.
    let notice = changed_notice(&files[..6]).unwrap();
    assert_eq!(
        notice.kind,
        Some(NoticeKind::FilesChangedNoCopy {
            files: files[..6].to_vec(),
            more: 0,
        })
    );
    let notice = gone_notice(&files[..6]).unwrap();
    assert_eq!(
        notice.kind,
        Some(NoticeKind::FilesGoneNoCopy {
            files: files[..6].to_vec(),
            more: 0,
        })
    );
}
