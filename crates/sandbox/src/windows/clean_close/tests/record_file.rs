//! The record's file: what is written reads back, and anything else is refused.

use super::*;

#[test]
fn a_written_record_reads_back_whole() {
    let s = Scene::new("clean-file-round");
    assert_eq!(s.close(), Closed::Written);
    let read = record::read(&s.data, &s.matter).unwrap().unwrap().unwrap();
    let bytes = record::render(&read).unwrap();
    assert_eq!(record::parse(&bytes).unwrap(), read);
    assert_eq!(read.header.inputs, WalkInputs::of_plan(&s.plan));
    assert_eq!(read.known.len(), 1, "one volume");
    // Three roots, each with its folder, its folder's file and 100 files.
    assert_eq!(read.known[0].len(), 3 * 103);
    assert!(read.known[0].root(100) && read.known[0].folder(101));
    assert!(!read.known[0].folder(102));
}

#[test]
fn another_version_a_short_file_or_another_matters_record_walks_in_full() {
    let s = Scene::new("clean-file-bad");
    assert_eq!(s.close(), Closed::Written);
    let path = record::record_path(&s.data, &s.matter);
    let good = std::fs::read(&path).unwrap();
    let mut other = good.clone();
    other[16] = 9;
    assert_eq!(record::parse(&other), Err(record::Unusable::Version));
    for bad in [
        &good[..good.len() - 1],
        &good[..20],
        b"not a record".as_slice(),
    ] {
        assert_eq!(record::parse(bad), Err(record::Unusable::Unreadable));
    }
    let mut longer = good.clone();
    longer.push(0);
    assert_eq!(record::parse(&longer), Err(record::Unusable::Unreadable));

    let mut parsed = record::parse(&good).unwrap();
    parsed.header.matter = crate::windows::test_world::matter_two();
    record::write(&s.data, &parsed).unwrap();
    let moved = record::record_path(&s.data, &parsed.header.matter);
    crate::windows::write::write_atomic(&path, &std::fs::read(&moved).unwrap()).unwrap();
    assert_eq!(s.launch_path(), LaunchPath::Full(FullReason::Unreadable));

    crate::windows::write::write_atomic(&path, &other).unwrap();
    assert_eq!(s.launch_path(), LaunchPath::Full(FullReason::Version));
}
