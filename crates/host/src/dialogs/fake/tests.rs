//! The fake answers in script order, records what it was asked, and refuses past the script.

use std::path::PathBuf;
use std::sync::Arc;

use super::FakeDialogs;
use crate::dialogs::{Dialog, DialogAnswer, NativeDialogs, Pick};

fn site(host: &str) -> Dialog {
    Dialog::Site {
        host: host.to_owned(),
        shared_hosting: false,
    }
}

#[test]
fn answers_in_order_then_refuses() {
    let fake = FakeDialogs::new([DialogAnswer::AllowForMatter, DialogAnswer::AllowOnce]);
    let dialogs: Arc<dyn NativeDialogs> = Arc::new(fake);
    assert_eq!(dialogs.ask(site("a.example")), DialogAnswer::AllowForMatter);
    assert_eq!(dialogs.ask(site("b.example")), DialogAnswer::AllowOnce);
    // Past the script, as an Escape: refused.
    assert_eq!(dialogs.ask(site("c.example")), DialogAnswer::Refuse);
}

#[test]
fn records_what_it_was_asked() {
    let fake = FakeDialogs::new([DialogAnswer::RefuseForMatter]);
    let folder = Dialog::Folder {
        matter_name: "Smith v Jones".to_owned(),
        path: PathBuf::from("/Users/rohan/Matters/Brown/brief.docx"),
        is_file: true,
        other_matter: Some("Brown".to_owned()),
    };
    assert_eq!(fake.ask(folder.clone()), DialogAnswer::RefuseForMatter);
    let off = Dialog::SwitchOff {
        matter_name: "Smith v Jones".to_owned(),
    };
    assert_eq!(fake.ask(off.clone()), DialogAnswer::Refuse);
    assert_eq!(fake.asked(), vec![folder, off]);
}

#[test]
fn picks_in_order_then_cancels() {
    let chosen = PathBuf::from("/Users/rohan/Matters/Brown");
    let fake = FakeDialogs::new([]).with_picks([Some(chosen.clone()), None]);
    let pick = Pick {
        start: PathBuf::from("/Users/rohan/Matters"),
        files_too: false,
        matter_name: "Smith v Jones".to_owned(),
    };
    assert_eq!(fake.pick(pick.clone()), Some(chosen));
    assert_eq!(fake.pick(pick.clone()), None);
    // Past the script: cancelled.
    assert_eq!(fake.pick(pick.clone()), None);
    assert_eq!(fake.picked(), vec![pick.clone(), pick.clone(), pick]);
    assert_eq!(fake.asked(), Vec::<Dialog>::new());
}
