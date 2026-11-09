//! The records the watch reads from its buffer: names made and renamed in, items that are
//! reparse points now, and which folders count gone by their check (renamed in, or moved in
//! from another folder as told by a creation time older than [`MOVED_IN_AGE`]).

use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use super::super::pairs::{PAIR_MAX, Pairs};
use super::super::records::{Layout, MOVED_IN_AGE, changes, filetime};

/// A record's action, name, attributes and creation time (a FILETIME).
type Item<'a> = (u32, &'a str, u32, i64);

/// When the watch read the buffer: 2022-06-13, as a FILETIME.
const NOW: u64 = 133_000_000_000_000_000;

/// `NOW` less `ticks`, as a creation time.
fn before(ticks: u64) -> i64 {
    i64::try_from(NOW - ticks).unwrap()
}

/// One record: its action, name, attributes and creation time, padded to a `u32` boundary, in
/// `layout`.
fn record(
    layout: Layout,
    (action, name, attributes, created): Item<'_>,
    id: u64,
    last: bool,
) -> Vec<u8> {
    let units: Vec<u8> = name.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let header = if layout == Layout::Plain { 12 } else { 84 };
    let size = (header + units.len()).next_multiple_of(4);
    let next = if last {
        0
    } else {
        u32::try_from(size).unwrap()
    };
    let mut bytes = vec![0; header];
    bytes[0..4].copy_from_slice(&next.to_le_bytes());
    bytes[4..8].copy_from_slice(&action.to_le_bytes());
    let length = u32::try_from(units.len()).unwrap().to_le_bytes();
    if layout == Layout::Plain {
        bytes[8..12].copy_from_slice(&length);
    } else {
        bytes[8..16].copy_from_slice(&created.to_le_bytes());
        bytes[56..60].copy_from_slice(&attributes.to_le_bytes());
        bytes[64..72].copy_from_slice(&id.to_le_bytes());
        bytes[80..84].copy_from_slice(&length);
    }
    bytes.extend(units);
    bytes.resize(size, 0);
    bytes
}

/// Records of items created just as the buffer was received.
fn buffer(layout: Layout, records: &[(u32, &str, u32)]) -> Vec<u8> {
    let items: Vec<Item<'_>> = records
        .iter()
        .map(|&(a, n, b)| (a, n, b, before(0)))
        .collect();
    buffer_at(layout, &items)
}

fn buffer_at(layout: Layout, records: &[Item<'_>]) -> Vec<u8> {
    let items: Vec<(Item<'_>, u64)> = records.iter().map(|each| (*each, 0)).collect();
    buffer_ids(layout, &items)
}

/// Records with their file IDs.
fn buffer_ids(layout: Layout, records: &[(Item<'_>, u64)]) -> Vec<u8> {
    let mut buffer = Vec::new();
    for (at, (each, id)) in records.iter().enumerate() {
        buffer.extend(record(layout, *each, *id, at + 1 == records.len()));
    }
    buffer
}

/// H1: names made or renamed to are read, and of the changes, only an item that is a reparse
/// point now (`FILE_ATTRIBUTE_REPARSE_POINT`, 0x400): a junction set on an empty folder.
#[test]
fn names_made_and_items_turned_into_reparse_points_are_read_from_the_buffer() {
    let records = [
        (1, ".git", 0x10),      // FILE_ACTION_ADDED
        (2, "gone", 0x10),      // REMOVED
        (3, "edited", 0x20),    // MODIFIED, an archive bit
        (3, "junction", 0x410), // MODIFIED, a folder that is a junction now
        (4, "old-name", 0x10),  // RENAMED_OLD_NAME
        (5, "sub/.GIT", 0x10),  // RENAMED_NEW_NAME
        (1, "foo", 0x20),
    ];
    let read = changes(&buffer(Layout::Extended, &records), Layout::Extended, NOW).unwrap();
    assert_eq!(read.added, [".git", "sub/.GIT", "foo"].map(PathBuf::from));
    assert_eq!(read.relinked, [PathBuf::from("junction")]);
    // Of the folders, only one renamed in: a folder made reports what goes in it.
    assert_eq!(read.moved_folders, [PathBuf::from("sub/.GIT")]);

    // With no attributes to tell by, every change is checked.
    let read = changes(&buffer(Layout::Plain, &records), Layout::Plain, NOW).unwrap();
    assert_eq!(read.added, [".git", "sub/.GIT", "foo"].map(PathBuf::from));
    assert_eq!(read.relinked, ["edited", "junction"].map(PathBuf::from));
    assert_eq!(read.moved_folders, Vec::<PathBuf>::new());
}

/// The fixes' review: a folder made (`mkdir tmp`, later `rm -r tmp`) is never listed to count
/// gone, since what goes in it is reported itself; one renamed in is.
#[test]
fn a_folder_just_made_is_never_listed_to_count_gone_and_one_renamed_in_is() {
    let made = [(1, "tmp", 0x10, before(0)), (2, "tmp", 0x10, before(0))];
    let read = changes(&buffer_at(Layout::Extended, &made), Layout::Extended, NOW).unwrap();
    assert_eq!(read.added, [PathBuf::from("tmp")]);
    assert_eq!(read.moved_folders, Vec::<PathBuf>::new());

    let renamed = [(4, "outside", 0x10), (5, "in", 0x10), (5, "note.md", 0x20)];
    let read = changes(&buffer(Layout::Extended, &renamed), Layout::Extended, NOW).unwrap();
    assert_eq!(read.moved_folders, [PathBuf::from("in")]);
}

#[test]
fn a_buffer_that_doesnt_read_as_records_is_an_overflow() {
    for layout in [Layout::Plain, Layout::Extended] {
        let whole = record(layout, (1, ".git", 0, 0), 0, true);
        assert_eq!(changes(&whole[..10], layout, NOW), None);
        let mut long = whole.clone();
        let at = if layout == Layout::Plain { 8 } else { 80 };
        long[at] = 200;
        assert_eq!(changes(&long, layout, NOW), None);
        let mut loops = record(layout, (1, "a", 0, 0), 0, false);
        loops[0..4].copy_from_slice(&256_u32.to_le_bytes());
        assert_eq!(changes(&loops, layout, NOW), None);
    }
}

/// The review: a folder moved in from another folder (`FILE_ACTION_ADDED`, keeping its creation
/// time) counts gone by its check, as one renamed in does; one just made (`mkdir`) doesn't.
#[test]
fn a_folder_moved_in_counts_gone_and_one_just_made_doesnt() {
    let moved_in = before(3600 * 10_000_000);
    let records = [
        (1, "from-temp", 0x10, moved_in), // moved in from the agent's temp
        (1, "made", 0x10, before(1_000_000)), // mkdir, 0.1 s ago
        (1, "old.txt", 0x20, moved_in),   // a file moved in: never a folder
        (5, "renamed", 0x10, moved_in),   // renamed in within its folder
        (5, "renamed-new", 0x10, before(0)), // renamed in, however new
    ];
    let read = changes(
        &buffer_at(Layout::Extended, &records),
        Layout::Extended,
        NOW,
    )
    .unwrap();
    assert_eq!(
        read.added,
        ["from-temp", "made", "old.txt", "renamed", "renamed-new"].map(PathBuf::from)
    );
    assert_eq!(
        read.moved_folders,
        ["from-temp", "renamed", "renamed-new"].map(PathBuf::from)
    );

    // The plain records have no times: nothing added counts.
    let read = changes(&buffer_at(Layout::Plain, &records), Layout::Plain, NOW).unwrap();
    assert_eq!(read.moved_folders, Vec::<PathBuf>::new());
}

/// At exactly [`MOVED_IN_AGE`] a folder added counts as made; one tick older, moved in.
#[test]
fn a_folder_added_counts_moved_in_only_past_moved_in_age() {
    assert_eq!(MOVED_IN_AGE, 5 * 10_000_000, "5 s in 100 ns ticks");
    let at_age = [(1, "at", 0x10, before(MOVED_IN_AGE))];
    let read = changes(&buffer_at(Layout::Extended, &at_age), Layout::Extended, NOW).unwrap();
    assert_eq!(read.moved_folders, Vec::<PathBuf>::new());
    let past = [(1, "past", 0x10, before(MOVED_IN_AGE + 1))];
    let read = changes(&buffer_at(Layout::Extended, &past), Layout::Extended, NOW).unwrap();
    assert_eq!(read.moved_folders, [PathBuf::from("past")]);
}

/// The VM review: a creation time after the buffer was received (clock skew, or forged), or none
/// (0, or negative), counts as moved in: the count fails toward ending the session.
#[test]
fn a_folder_created_in_the_future_or_with_no_time_counts_as_moved_in() {
    let future = i64::try_from(NOW + 3600 * 10_000_000).unwrap();
    for created in [future, i64::try_from(NOW + 1).unwrap(), 0, -1, i64::MIN] {
        let records = [(1, "odd", 0x10, created)];
        let read = changes(
            &buffer_at(Layout::Extended, &records),
            Layout::Extended,
            NOW,
        )
        .unwrap();
        assert_eq!(read.moved_folders, [PathBuf::from("odd")], "{created}");
    }
    // A clock that can't be read (0) counts every folder added.
    let records = [(1, "new", 0x10, before(0))];
    let read = changes(&buffer_at(Layout::Extended, &records), Layout::Extended, 0).unwrap();
    assert_eq!(read.moved_folders, [PathBuf::from("new")]);
}

/// The wall clock as a FILETIME: 100 ns ticks since 1601.
#[test]
fn the_wall_clock_reads_as_a_filetime() {
    assert_eq!(filetime(UNIX_EPOCH), 116_444_736_000_000_000);
    let later = UNIX_EPOCH + Duration::from_secs(1) + Duration::from_nanos(250);
    assert_eq!(filetime(later), 116_444_736_010_000_002);
    assert_eq!(filetime(UNIX_EPOCH - Duration::from_secs(1)), 0);
}

/// The ruling's pairing: a folder made just now is listed fresh with its file ID (an ADDED one
/// with no file ID isn't), and each folder removed or renamed away is listed with its own; a
/// file isn't, and an item with no attributes at all is, in case it was a folder.
#[test]
fn fresh_folders_and_what_left_are_read_with_their_file_ids() {
    let records = [
        ((1, "made", 0x10, before(0)), 7),
        ((1, "no-id", 0x10, before(0)), 0),
        ((1, "file", 0x20, before(0)), 8),
        ((1, "old", 0x10, before(MOVED_IN_AGE + 1)), 9),
        ((2, "rm", 0x10, 0), 10),
        ((4, "away", 0x10, 0), 11),
        ((4, "file-away", 0x20, 0), 12),
        ((2, "file-gone", 0x80, 0), 13),
        ((2, "unknown", 0, 0), 14),
        ((2, "no-id-gone", 0x10, 0), 0),
    ];
    let read = changes(
        &buffer_ids(Layout::Extended, &records),
        Layout::Extended,
        NOW,
    )
    .unwrap();
    assert_eq!(read.fresh_folders, [(PathBuf::from("made"), 7)]);
    assert_eq!(read.moved_folders, [PathBuf::from("old")]);
    assert_eq!(
        read.left,
        [
            (PathBuf::from("rm"), 10),
            (PathBuf::from("away"), 11),
            (PathBuf::from("unknown"), 14)
        ]
    );
    let read = changes(&buffer_ids(Layout::Plain, &records), Layout::Plain, NOW).unwrap();
    assert_eq!(read.fresh_folders, Vec::<(PathBuf, u64)>::new());
    assert_eq!(read.left, Vec::<(PathBuf, u64)>::new());
}

/// The review's flood: the agent moves a folder in from its temp folder, deletes about 5000
/// files there, then moves the folder on. The files' records aren't listed, so the folder's
/// leaving record stays in the pairing's list and the gone folder pairs, with nothing lost.
#[test]
fn a_flood_of_files_leaving_temp_pushes_no_folder_out_of_the_pairing() {
    let names: Vec<String> = (0..5000).map(|n| format!("f{n}.tmp")).collect();
    let mut records = vec![((2_u32, "x", 0x10_u32, 0_i64), 7_u64)];
    records.extend(names.iter().map(|name| ((2, name.as_str(), 0x20, 0), 100)));
    assert!(records.len() > PAIR_MAX);
    let read = changes(
        &buffer_ids(Layout::Extended, &records),
        Layout::Extended,
        NOW,
    )
    .unwrap();
    assert_eq!(read.left, [(PathBuf::from("x"), 7)]);
    let pairs = Pairs::default();
    let now = Duration::from_secs(100);
    pairs.note(now, Path::new("temp"), &read.left);
    assert!(!pairs.lost(now));
    assert!(pairs.paired(now, 7, &Path::new("matter").join("x")));
}
