//! The sandbox folder's DACL is written on the folder alone, on real NTFS in a temp folder (no
//! account needed): an existing child keeps its own descriptor, a junction in the folder and what
//! it names are untouched, and what is made inside afterwards inherits the folder's entries.

use crate::windows::acl::descriptor;
use crate::windows::acl_bytes::INHERITED;
use crate::windows::record::current_user_sid;
use crate::windows::test_acl::{Place, entries, protected};
use crate::windows::test_dacl::protect_with;
use crate::windows::test_junction::junction;

#[test]
fn securing_a_folder_writes_it_alone_and_what_is_made_after_inherits() {
    let place = Place::new("private-dir-alone");
    place.dir.file("canary/child/keep.txt", "kept");
    let canary = place.at("canary");
    protect_with(
        &canary,
        "D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FA;;;OW)(A;OICI;FR;;;WD)",
    )
    .unwrap();
    let watched = [
        canary.clone(),
        canary.join("child"),
        canary.join("child").join("keep.txt"),
    ];
    let before: Vec<Vec<u8>> = watched.iter().map(|p| descriptor(p).unwrap()).collect();
    place.dir.file("data/sandbox/old/old.txt", "old");
    let folder = place.at("data/sandbox");
    let link = folder.join("planted");
    junction(&link, &canary).unwrap();
    let kept = [folder.join("old"), folder.join("old").join("old.txt"), link];
    let kept_before: Vec<Vec<u8>> = kept.iter().map(|p| descriptor(p).unwrap()).collect();

    super::protect(&folder).unwrap();

    assert!(protected(&folder));
    let after: Vec<Vec<u8>> = watched.iter().map(|p| descriptor(p).unwrap()).collect();
    assert_eq!(after, before, "what the junction names is untouched");
    let kept_after: Vec<Vec<u8>> = kept.iter().map(|p| descriptor(p).unwrap()).collect();
    assert_eq!(kept_after, kept_before, "nothing already inside changed");
    // What is made now inherits the folder's three entries, and only them.
    place.dir.file("data/sandbox/new/new.txt", "new");
    let me = current_user_sid().unwrap();
    for made in [folder.join("new"), folder.join("new").join("new.txt")] {
        let mut inherited: Vec<String> = entries(&made)
            .into_iter()
            .filter(|(_, flags, _, _)| flags & INHERITED != 0)
            .map(|(_, _, _, sid)| sid.as_str().to_owned())
            .collect();
        inherited.sort();
        inherited.dedup();
        let mut expected = vec![me.as_str().to_owned(), "S-1-5-18".to_owned()];
        expected.push("S-1-5-32-544".to_owned());
        expected.sort();
        assert_eq!(inherited, expected, "{}", made.display());
    }
}
