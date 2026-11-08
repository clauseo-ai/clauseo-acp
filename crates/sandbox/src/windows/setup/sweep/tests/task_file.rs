//! A task the Task Scheduler won't give is read from its task file: skipped only when the file
//! names a principal that isn't the pool's; a pool principal, a file that can't be read, is
//! malformed or is over the cap, and a path part that isn't a plain name all stay 56.

use super::*;
use crate::windows::setup::sweep::task_file::{parts, principal_of};

/// A task file as the Task Scheduler writes it: UTF-16LE after its byte order mark, its
/// principal `inner`.
fn file(inner: &str) -> Vec<u8> {
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-16\"?>\r\n<Task version=\"1.2\" \
         xmlns=\"http://schemas.microsoft.com/windows/2004/02/mit/task\">\r\n  \
         <RegistrationInfo><Author>PC-7\\x</Author></RegistrationInfo>\r\n  <Principals>\r\n    \
         <Principal id=\"Author\">{inner}<LogonType>InteractiveToken</LogonType></Principal>\r\n  \
         </Principals>\r\n  <Actions Context=\"Author\"><Exec><Command>cmd.exe</Command>\
         <Arguments>/c echo &lt;a&gt; &amp; exit</Arguments></Exec></Actions>\r\n</Task>\r\n"
    );
    let mut bytes = vec![0xFF, 0xFE];
    bytes.extend(xml.encode_utf16().flat_map(u16::to_le_bytes));
    bytes
}

/// A fake of pool user 1010, with the task `name` in `\A\B` that the Task Scheduler won't give,
/// and its file `bytes`.
fn hidden(name: &str, bytes: Result<Vec<u8>, Failed>) -> Fake {
    let mut fake = Fake::new(&[1010]);
    fake.names.insert(sid(1010), "ClauseoSb01".to_owned());
    fake.unread("\\A\\B", name);
    fake.files.insert(format!("A\\B\\{name}"), bytes);
    fake
}

#[test]
fn a_task_file_naming_a_principal_not_the_pools_is_skipped() {
    let mut fake = Fake::new(&[1010]);
    fake.accounts
        .insert(r"PC-7\lawyer".to_owned(), Sid::parse(OWNER).unwrap());
    for (name, inner) in [
        ("system", "<UserId>S-1-5-18</UserId>"),
        ("lawyer", "<UserId>PC-7\\lawyer</UserId>"),
        ("users", "<GroupId>S-1-5-32-545</GroupId>"),
        ("domain", "<UserId>CORP\\alice</UserId>"),
    ] {
        fake.unread("\\A\\B", name);
        fake.files.insert(format!("A\\B\\{name}"), Ok(file(inner)));
    }
    assert_eq!(sweep(&mut fake), SweepExit::Done);
    assert_eq!(fake.deleted(), Vec::<&str>::new());
    let line = &fake.logged[0].1;
    assert!(
        line.contains("0 unreadable (4 more skipped, not the pool's"),
        "{line}"
    );
}

#[test]
fn a_task_file_naming_a_pool_user_or_no_one_known_stays_56() {
    for inner in [
        "<UserId>S-1-5-21-1-2-3-1010</UserId>",
        "<UserId>s-1-0x5-21-1-2-3-1010</UserId>",
        "<UserId>ClauseoSb01</UserId>",
        "<UserId>deleted-user</UserId>",
        "<GroupId>  </GroupId>",
        "",
    ] {
        let mut fake = hidden("t", Ok(file(inner)));
        assert_eq!(sweep(&mut fake), SweepExit::TasksLeft, "{inner}");
        assert_eq!(fake.deleted(), Vec::<&str>::new(), "{inner}");
        assert!(fake.done.contains(&r"file A\B\t".to_owned()));
        let line = &fake.logged[0].1;
        assert!(
            line.contains("1 unreadable (0 more skipped"),
            "{inner}: {line}"
        );
    }
}

#[test]
fn a_task_file_that_cant_be_read_or_is_malformed_stays_56() {
    let mut big = file("<UserId>S-1-5-18</UserId>");
    big.resize(TASK_FILE_CAP + 2, 0x20);
    let utf8 = |xml: &str| Ok(xml.as_bytes().to_vec());
    let cases: Vec<(&str, Result<Vec<u8>, Failed>)> = vec![
        ("missing", Err("the task file is missing".to_owned())),
        ("over the cap", Ok(big)),
        ("odd UTF-16", Ok(vec![0xFF, 0xFE, b'<'])),
        (
            "a DTD",
            utf8(
                "<!DOCTYPE Task [<!ENTITY u \"S-1-5-18\">]><Task><Principals>\
            <Principal><UserId>&u;</UserId></Principal></Principals></Task>",
            ),
        ),
        (
            "an entity",
            utf8(
                "<Task><Principals><Principal><UserId>&u;</UserId></Principal>\
            </Principals></Task>",
            ),
        ),
        (
            "unended",
            utf8("<Task><Principals><Principal><UserId>S-1-5-18</UserId>"),
        ),
        (
            "mismatched",
            utf8(
                "<Task><Principals><Principal><UserId>S-1-5-18</GroupId>\
            </Principal></Principals></Task>",
            ),
        ),
        (
            "two principals",
            Ok(file(
                "<UserId>S-1-5-18</UserId></Principal><Principal>\
            <UserId>S-1-5-21-1-2-3-1010</UserId>",
            )),
        ),
        (
            "two users",
            Ok(file("<UserId>S-1-5-18</UserId><UserId>S-1-5-18</UserId>")),
        ),
        (
            "text after",
            utf8(
                "<Task><Principals><Principal><UserId>S-1-5-18</UserId>\
            </Principal></Principals></Task>x",
            ),
        ),
        ("two roots", utf8("<Task></Task><Task></Task>")),
        (
            "not a task",
            utf8(
                "<Job><Principals><Principal><UserId>S-1-5-18</UserId>\
            </Principal></Principals></Job>",
            ),
        ),
        ("a child", Ok(file("<UserId>S-1-5-18<x/></UserId>"))),
        ("not text", Ok(vec![0xFE, 0xFF, 0, b'<'])),
    ];
    for (case, bytes) in cases {
        let mut fake = hidden("t", bytes);
        assert_eq!(sweep(&mut fake), SweepExit::TasksLeft, "{case}");
        let line = &fake.logged[0].1;
        assert!(
            line.contains("1 unreadable (0 more skipped"),
            "{case}: {line}"
        );
    }
}

#[test]
fn a_task_file_path_part_that_isnt_a_plain_name_is_never_read() {
    for name in ["..", "a..b", "c:d", "e/f", "g\\h", ".", "", "i\u{1}", "j*"] {
        let mut fake = Fake::new(&[1010]);
        fake.unread("\\A", name);
        assert_eq!(sweep(&mut fake), SweepExit::TasksLeft, "{name:?}");
        let read = fake.done.iter().any(|done| done.starts_with("file "));
        assert!(!read, "{name:?}: {:?}", fake.done);
    }
    assert_eq!(parts("\\", "t").unwrap(), ["t"]);
    assert_eq!(parts("\\A\\B", "t").unwrap(), ["A", "B", "t"]);
    for folder in ["", "A", "\\A\\..\\B", "\\A\\\\B", "\\A:"] {
        assert!(parts(folder, "t").is_err(), "{folder:?}");
    }
}

#[test]
fn the_task_files_principal_is_read_narrowly() {
    let principal = principal_of(&file(
        "<!-- a note --><UserId> PC-7\\Clauseo&#83;b01 </UserId><GroupId>S-1-5-4</GroupId>",
    ))
    .unwrap();
    assert_eq!(principal.user_id, "PC-7\\ClauseoSb01");
    assert_eq!(principal.group_id, "S-1-5-4");
    // UTF-8, with or without its mark; a UserId elsewhere is never read.
    let xml = "<Task><Data><UserId>S-1-5-21-1-2-3-1010</UserId></Data><Principals><Principal>\
               <UserId><![CDATA[S-1-5-18]]></UserId></Principal></Principals></Task>";
    let mut marked = vec![0xEF, 0xBB, 0xBF];
    marked.extend(xml.as_bytes());
    for bytes in [xml.as_bytes().to_vec(), marked] {
        assert_eq!(principal_of(&bytes).unwrap().user_id, "S-1-5-18");
    }
}
