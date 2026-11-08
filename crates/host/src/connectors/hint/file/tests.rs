//! `hints.json`: marks persist across a new `Hints`, a damaged file counts as no marks, and a
//! later version's mark is kept.

use clauseo_protocol::MatterId;
use uuid::Uuid;

use super::{FILE, Hints, Mark};
use crate::pipeline::tests::support::TempDir;

fn matter() -> MatterId {
    MatterId::new(Uuid::now_v7()).unwrap()
}

fn scratch(name: &str) -> TempDir {
    TempDir::new(&format!("hints-{name}"))
}

#[test]
fn a_mark_is_written_and_read_back() {
    let dir = scratch("mark");
    let data = dir.path();
    let (shown, dismissed, other) = (matter(), matter(), matter());
    let mut hints = Hints::new(data);
    assert!(!hints.marked(shown));
    hints.mark(shown, Mark::Shown).unwrap();
    hints.mark(dismissed, Mark::Dismissed).unwrap();
    let text = std::fs::read_to_string(data.join("connectors").join(FILE)).unwrap();
    let disk: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(disk["v"], 1);
    assert_eq!(disk["matters"][shown.to_string()], "shown");
    assert_eq!(disk["matters"][dismissed.to_string()], "dismissed");
    let mut again = Hints::new(data);
    assert!(again.marked(shown) && again.marked(dismissed) && !again.marked(other));
}

#[test]
fn a_damaged_or_other_version_file_counts_as_no_marks() {
    let dir = scratch("damaged");
    let data = dir.path();
    let folder = data.join("connectors");
    crate::write::create_dir_all(&folder).unwrap();
    let one = matter();
    for text in [
        "not json".to_owned(),
        format!(r#"{{"v":2,"matters":{{"{one}":"shown"}}}}"#),
    ] {
        crate::write::replace_private_file(&folder.join(FILE), text.as_bytes()).unwrap();
        assert!(!Hints::new(data).marked(one));
    }
}

#[test]
fn a_mark_of_a_later_value_counts_and_is_kept() {
    let dir = scratch("later");
    let data = dir.path();
    let folder = data.join("connectors");
    crate::write::create_dir_all(&folder).unwrap();
    let (later, new) = (matter(), matter());
    let text = format!(r#"{{"v":1,"matters":{{"{later}":"snoozed"}}}}"#);
    crate::write::replace_private_file(&folder.join(FILE), text.as_bytes()).unwrap();
    let mut hints = Hints::new(data);
    assert!(hints.marked(later));
    hints.mark(new, Mark::Shown).unwrap();
    let text = std::fs::read_to_string(folder.join(FILE)).unwrap();
    assert!(text.contains(&format!(r#""{later}":"snoozed""#)), "{text}");
}
