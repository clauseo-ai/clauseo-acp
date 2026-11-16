use std::cell::RefCell;
use std::path::{Path, PathBuf};

use super::{expand, long_name, may_be_short, with_room};

/// A relative path from its parts, so the tests hold no absolute Unix path.
fn path(parts: &[&str]) -> PathBuf {
    parts.iter().collect()
}

#[test]
fn only_a_path_with_a_tilde_may_hold_a_short_name() {
    assert!(may_be_short(&path(&["Users", "LONGPA~1", "Cases"])));
    assert!(!may_be_short(&path(&["Users", "longname", "Cases"])));
    assert!(!may_be_short(Path::new("")));
}

#[test]
fn a_path_without_a_tilde_comes_back_as_it_is() {
    let plain = path(&["Users", "longname", "Cases"]);
    assert_eq!(long_name(&plain), plain);
}

#[test]
fn the_path_itself_is_spelt_out_when_it_exists() {
    let short = path(&["Users", "LONGPA~1", "Cases"]);
    let long = path(&["Users", "longpathname", "Cases"]);
    let found = expand(&short, |asked| (asked == short).then(|| long.clone()));
    assert_eq!(found, Some(long));
}

#[test]
fn a_missing_folder_keeps_its_parts_below_the_longest_existing_ancestor() {
    let short = path(&["Users", "LONGPA~1", "Temp", "New~1", "Acme"]);
    let asked = RefCell::new(Vec::new());
    let found = expand(&short, |at| {
        asked.borrow_mut().push(at.to_path_buf());
        (at == path(&["Users", "LONGPA~1", "Temp"]))
            .then(|| path(&["Users", "longpathname", "Temp"]))
    });
    assert_eq!(
        found,
        Some(path(&["Users", "longpathname", "Temp", "New~1", "Acme"]))
    );
    assert_eq!(
        asked.into_inner(),
        [
            short,
            path(&["Users", "LONGPA~1", "Temp", "New~1"]),
            path(&["Users", "LONGPA~1", "Temp"]),
        ]
    );
}

#[test]
fn the_search_stops_above_the_last_short_name() {
    let short = path(&["Users", "LONGPA~1", "Cases"]);
    let asked = RefCell::new(Vec::new());
    let found = expand(&short, |at| {
        asked.borrow_mut().push(at.to_path_buf());
        None
    });
    assert_eq!(found, None);
    assert_eq!(asked.into_inner(), [short, path(&["Users", "LONGPA~1"])]);
}

#[test]
fn a_path_ending_in_a_part_with_no_name_is_not_spelt_out() {
    let short = path(&["LONGPA~1", ".."]);
    assert_eq!(expand(&short, |_| None), None);
}

#[test]
fn the_room_grows_to_the_length_asked() {
    let answer: Vec<u16> = (1..=300).collect();
    let mut sizes = Vec::new();
    let found = with_room(|room| {
        sizes.push(room.len());
        if room.len() <= answer.len() {
            return u32::try_from(answer.len() + 1).unwrap();
        }
        for (slot, unit) in room.iter_mut().zip(&answer) {
            *slot = *unit;
        }
        u32::try_from(answer.len()).unwrap()
    });
    assert_eq!(found, Some(answer));
    assert_eq!(sizes, [260, 302]);
}

#[test]
fn a_failed_or_ever_growing_call_gives_nothing() {
    assert_eq!(with_room(|_| 0), None);
    let mut calls = 0;
    let found = with_room(|room| {
        calls += 1;
        u32::try_from(room.len() * 2).unwrap()
    });
    assert_eq!(found, None);
    assert_eq!(calls, 3);
}

#[cfg(windows)]
#[test]
fn a_real_short_name_is_spelt_out() {
    let long = tempfile::Builder::new()
        .prefix("clauseo a folder with a long name ")
        .tempdir()
        .unwrap();
    // 8.3 names may be off on this volume.
    let Some(short) = super::windows::os_short_name(long.path()).filter(|s| may_be_short(s)) else {
        return;
    };
    let real = long_name(long.path());
    assert_eq!(long_name(&short), real);
    assert_eq!(long_name(&short.join("missing")), real.join("missing"));
}
