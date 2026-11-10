//! The home folder in a `TurnFailed.detail` (`scrub_home`): every spelling the host keeps, as
//! it is, with forward slashes, as a `file://` URL and JSON-escaped, ignoring case.

use std::path::PathBuf;

use super::scrub_home;

#[test]
fn every_spelling_of_the_home_folder_is_written_as_a_tilde() {
    let homes = [
        PathBuf::from(r"C:\Users\Asha"),
        PathBuf::from("/Users/asha"),
        PathBuf::from("/private/var/home/asha"),
    ];
    let detail = [
        r"at C:\Users\Asha\matter\a.docx",
        r"at c:\users\asha\b.docx",
        "at C:/Users/Asha/c.docx",
        "at file:///C:/Users/Asha/d.docx",
        r#"{"path":"C:\\Users\\Asha\\e.docx"}"#,
        "at /Users/asha/f.docx",
        "at file:///Users/asha/g.docx",
        "at /private/var/home/asha/h.docx",
    ]
    .join("; ");
    let scrubbed = scrub_home(detail, &homes);
    assert_eq!(
        scrubbed,
        [
            r"at ~\matter\a.docx",
            r"at ~\b.docx",
            "at ~/c.docx",
            "at ~/d.docx",
            r#"{"path":"~\\e.docx"}"#,
            "at ~/f.docx",
            "at ~/g.docx",
            "at ~/h.docx",
        ]
        .join("; ")
    );
    assert!(!scrubbed.to_ascii_lowercase().contains("asha"));
}

#[test]
fn a_root_home_and_no_home_leave_the_text_alone() {
    let text = "connect failed at /tmp/x".to_owned();
    assert_eq!(scrub_home(text.clone(), &[PathBuf::from("/")]), text);
    assert_eq!(scrub_home(text.clone(), &[]), text);
}
