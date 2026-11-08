//! The consumer's edges stage F and the orchestrator asked for: stamps, versions, SIDs that
//! almost match, the SID item's glue, the 32-bit layout, odd and long names, network paths,
//! ETW's own losses and the stale key.

use std::time::Duration;

use super::super::consumer::{
    ACCESS_DENIED, CREATE_ID, END_ID, EXT_TYPE_SID, MAX_NAME_UNITS, RawEvent, known_version,
    parse_create, sid_bytes, sid_in,
};
use super::super::losses::{Losses, QUERY_EVERY};
use super::super::paths::DeviceMap;
use super::super::protocol::{KeyStart, Line, denied_line, key_start};
use super::{Fed, LAWYER, POOL_1, POOL_2, create_data, devices, end_data, local, net, pool, sid};

const NAME: &str = r"\Device\HarddiskVolume3\f";

/// A pool Create stamped `stamp`.
fn create_at(fed: &mut Fed, stamp: i64, irp: u64, name: &str) {
    let bytes = sid_bytes(&sid(POOL_1)).unwrap();
    let data = create_data(irp, 0x0100_0000, name);
    at(fed, CREATE_ID, 1, stamp, Some(&bytes), &data);
}

/// An `OperationEnd` stamped `stamp`.
fn end_at(fed: &mut Fed, stamp: i64, irp: u64, status: u32) {
    at(fed, END_ID, 0, stamp, None, &end_data(irp, status));
}

/// Event `id` of `version`, stamped `stamp`.
fn at(fed: &mut Fed, id: u16, version: u8, stamp: i64, sid: Option<&[u8]>, data: &[u8]) {
    fed.raw(&RawEvent {
        id,
        pid: 7,
        sid,
        data,
        wide: true,
        version,
        stamp,
    });
}

#[test]
fn an_end_stamped_before_its_create_pairs_with_nothing() {
    let mut fed = Fed::new();
    create_at(&mut fed, 10, 0x10, NAME);
    // The end of the IRP's earlier use, delivered late: the Create keeps waiting.
    end_at(&mut fed, 5, 0x10, ACCESS_DENIED);
    assert_eq!(fed.out, []);
    assert_eq!(fed.consumer.pending(), 1);
    end_at(&mut fed, 15, 0x10, ACCESS_DENIED);
    assert_eq!(fed.out.len(), 1);
    assert_eq!(fed.consumer.pending(), 0);
}

#[test]
fn a_reused_irp_after_a_lost_end_pairs_the_later_create() {
    let mut fed = Fed::new();
    create_at(&mut fed, 1, 0x20, r"\Device\HarddiskVolume3\first");
    // Its end was lost; the IRP's next Create replaces it, and an end older than that is not
    // its own.
    create_at(&mut fed, 3, 0x20, r"\Device\HarddiskVolume3\second");
    end_at(&mut fed, 2, 0x20, ACCESS_DENIED);
    assert_eq!(fed.out, []);
    end_at(&mut fed, 4, 0x20, ACCESS_DENIED);
    let paths: Vec<_> = fed.out.iter().map(|r| r.path.as_str()).collect();
    assert_eq!(paths, [r"C:\second"]);
}

#[test]
fn a_sid_that_almost_matches_a_pool_users_is_no_match() {
    let mut fed = Fed::new();
    let pool_1 = sid_bytes(&sid(POOL_1)).unwrap();
    // One more sub-authority (the count says so), and the same SID cut short.
    let mut longer = sid_bytes(&sid(&format!("{POOL_1}-5"))).unwrap();
    let truncated = &pool_1[..pool_1.len() - 1];
    let mut appended = pool_1.clone();
    appended.extend_from_slice(&5u32.to_le_bytes());
    for (n, bytes) in [longer.clone(), truncated.to_vec(), appended]
        .iter()
        .enumerate()
    {
        let irp = 0x100 + n as u64;
        let data = create_data(irp, 0x0100_0000, NAME);
        fed.feed(CREATE_ID, 1, Some(bytes), &data);
        fed.end(irp, ACCESS_DENIED);
    }
    longer.clear();
    fed.feed(CREATE_ID, 1, Some(&longer), &create_data(0x200, 0, NAME));
    fed.end(0x200, ACCESS_DENIED);
    assert_eq!(fed.out, []);
    assert_eq!(fed.consumer.counts().kept, 0);
}

#[test]
fn the_sid_item_glue_reads_only_the_sid_item_and_a_non_pool_one_never_reaches_the_output() {
    let lawyer = sid_bytes(&sid(LAWYER)).unwrap();
    let pool_1 = sid_bytes(&sid(POOL_1)).unwrap();
    // A pool SID's bytes in another item (a terminal session's, say) are never the SID.
    let items = [
        (3u16, Some(pool_1.as_slice())),
        (EXT_TYPE_SID, Some(lawyer.as_slice())),
    ];
    assert_eq!(sid_in(items), Some(lawyer.as_slice()));
    assert_eq!(sid_in([(3u16, Some(pool_1.as_slice()))]), None);
    assert_eq!(
        sid_in([(EXT_TYPE_SID, None), (3, Some(pool_1.as_slice()))]),
        None
    );
    assert_eq!(sid_in([]), None);
    // Through the consumer: the lawyer's Create, and one whose only pool bytes are elsewhere.
    let mut fed = Fed::new();
    let data = create_data(0x10, 0x0100_0000, r"\Device\HarddiskVolume3\private.txt");
    fed.feed(CREATE_ID, 1, sid_in(items), &data);
    fed.end(0x10, ACCESS_DENIED);
    let elsewhere = [(3u16, Some(pool_1.as_slice()))];
    fed.feed(CREATE_ID, 1, sid_in(elsewhere), &create_data(0x11, 0, NAME));
    fed.end(0x11, ACCESS_DENIED);
    assert_eq!(fed.out, []);
    assert_eq!(fed.consumer.counts().kept, 0);
    // The pool user's own item passes, and its line is the pool user's.
    let mine = [(EXT_TYPE_SID, Some(pool_1.as_slice()))];
    fed.feed(
        CREATE_ID,
        1,
        sid_in(mine),
        &create_data(0x12, 0x0100_0000, NAME),
    );
    fed.end(0x12, ACCESS_DENIED);
    assert_eq!(fed.out.len(), 1);
    assert!(denied_line(&fed.out[0], &pool()).is_some());
}

#[test]
fn a_32_bit_create_reads_at_its_own_offsets() {
    let mut data = 0xabcu32.to_le_bytes().to_vec();
    data.extend_from_slice(&0x1000u32.to_le_bytes()); // FileObject
    data.extend_from_slice(&42u32.to_le_bytes()); // IssuingThreadId
    data.extend_from_slice(&0x0300_0040u32.to_le_bytes()); // CreateOptions
    data.extend_from_slice(&0x80u32.to_le_bytes());
    data.extend_from_slice(&7u32.to_le_bytes());
    for unit in r"\Device\HarddiskVolume3\x".encode_utf16().chain([0]) {
        data.extend_from_slice(&unit.to_le_bytes());
    }
    let create = parse_create(&data, false).unwrap();
    assert_eq!((create.irp, create.options), (0xabc, 0x0300_0040));
    assert_eq!(create.file_name, r"\Device\HarddiskVolume3\x");
}

#[test]
fn a_name_of_an_odd_byte_count_is_nothing() {
    let mut data = create_data(1, 0, "ab");
    data.push(0);
    assert_eq!(parse_create(&data, true), None);
}

#[test]
fn a_name_over_the_limit_is_dropped_and_counted_not_truncated() {
    let mut fed = Fed::new();
    let prefix = r"\Device\HarddiskVolume3\";
    let long = format!("{prefix}{}", "a".repeat(MAX_NAME_UNITS));
    create_at(&mut fed, 1, 0x30, NAME);
    create_at(&mut fed, 2, 0x30, &long);
    end_at(&mut fed, 3, 0x30, ACCESS_DENIED);
    // Not created, and the IRP's earlier Create isn't paired with this open's end either.
    assert_eq!(fed.out, []);
    assert_eq!(fed.consumer.counts().too_long, 1);
    assert_eq!(fed.consumer.take_dropped(), 1);
    assert_eq!(fed.consumer.take_dropped(), 0);
    // At the limit is kept.
    let at_limit = format!("{prefix}{}", "a".repeat(MAX_NAME_UNITS - prefix.len()));
    assert_eq!(at_limit.encode_utf16().count(), MAX_NAME_UNITS);
    create_at(&mut fed, 4, 0x31, &at_limit);
    end_at(&mut fed, 5, 0x31, ACCESS_DENIED);
    assert_eq!(fed.out.len(), 1);
}

#[test]
fn only_the_known_versions_are_read_and_the_rest_counted() {
    assert!(known_version(CREATE_ID, 1) && known_version(END_ID, 0));
    assert!(!known_version(CREATE_ID, 0) && !known_version(CREATE_ID, 2));
    assert!(!known_version(END_ID, 1));
    let mut fed = Fed::new();
    let bytes = sid_bytes(&sid(POOL_1)).unwrap();
    at(
        &mut fed,
        CREATE_ID,
        2,
        1,
        Some(&bytes),
        &create_data(0x40, 0x0100_0000, NAME),
    );
    assert_eq!(fed.consumer.pending(), 0);
    create_at(&mut fed, 2, 0x41, NAME);
    at(&mut fed, END_ID, 1, 3, None, &end_data(0x41, ACCESS_DENIED));
    assert_eq!((fed.out.len(), fed.consumer.pending()), (0, 1));
    // Another event id isn't counted: only 12 and 24 reach the consumer as Kernel-File's.
    at(&mut fed, 13, 9, 4, None, &[]);
    assert_eq!(fed.consumer.counts().unknown_version, 2);
    end_at(&mut fed, 5, 0x41, ACCESS_DENIED);
    assert_eq!(fed.out.len(), 1);
}

#[test]
fn network_and_unknown_device_paths_are_flagged_and_local_ones_are_not() {
    let mut map = DeviceMap::default();
    let at = Duration::from_secs(10);
    let mut to = |path: &str| map.map_path(path, at, &mut devices);
    assert_eq!(
        to(r"\Device\Mup\server\share\f.txt"),
        Some(net(r"\\server\share\f.txt"))
    );
    assert_eq!(
        to(r"\Device\LanmanRedirector\;Z:00000000000a1b2c\server\share\f.txt"),
        Some(net(r"\\server\share\f.txt"))
    );
    assert_eq!(
        to(r"\Device\Mup\;LanmanRedirector\;Z:00000000000a1b2c\server\share\f"),
        Some(net(r"\\server\share\f"))
    );
    assert_eq!(
        to(r"\Device\WebDavRedirector\;Y:0000000000001234\host@SSL\DavWWWRoot\d"),
        Some(net(r"\\host@SSL\DavWWWRoot\d"))
    );
    assert_eq!(
        to(r"\Device\Mup\;WebDavRedirector\host\DavWWWRoot\d"),
        Some(net(r"\\host\DavWWWRoot\d"))
    );
    // Any \;X: form, whatever its provider.
    assert_eq!(
        to(r"\Device\SomeRedirector\;Q:0000000000000001\srv\shr\x"),
        Some(net(r"\\srv\shr\x"))
    );
    // A device not known as a local volume shows as its NT path, flagged.
    let unknown = r"\Device\SomethingElse\x\y";
    assert_eq!(to(unknown), Some(net(unknown)));
    // A redirector path without both a server and a share shows as its NT path.
    assert_eq!(to(r"\Device\Mup\server"), Some(net(r"\Device\Mup\server")));
    // A local volume stays unflagged.
    assert_eq!(
        to(r"\Device\HarddiskVolume3\Users\a"),
        Some(local(r"C:\Users\a"))
    );
    // A drive letter on a share is never a local volume.
    let mut map = DeviceMap::default();
    let mut share = || {
        vec![(
            'Z',
            r"\Device\LanmanRedirector\;Z:0000000000000001\s\h".to_owned(),
        )]
    };
    assert_eq!(
        map.map_path(
            r"\Device\LanmanRedirector\;Z:0000000000000001\s\h\f",
            at,
            &mut share
        ),
        Some(net(r"\\s\h\f"))
    );
}

#[test]
fn a_network_refusal_goes_out_with_net_and_a_local_one_without() {
    let mut fed = Fed::new();
    fed.create(POOL_2, 3, 0x50, 0x0100_0000, r"\Device\Mup\evil\share\x");
    fed.end(0x50, ACCESS_DENIED);
    fed.create(POOL_2, 3, 0x51, 0x0100_0000, NAME);
    fed.end(0x51, ACCESS_DENIED);
    let lines: Vec<String> = fed
        .out
        .iter()
        .filter_map(|r| denied_line(r, &pool()))
        .collect();
    assert!(lines[0].contains("\"net\":true"), "{}", lines[0]);
    assert!(lines[1].contains("\"net\":false"), "{}", lines[1]);
    assert!(matches!(
        Line::parse(lines[0].trim_end()),
        Some(Line::Denied { net: true, .. })
    ));
    // Sent as false, a local line reads as local.
    assert!(matches!(
        Line::parse(lines[1].trim_end()),
        Some(Line::Denied { net: false, .. })
    ));
}

#[test]
fn etws_own_losses_are_counted_by_how_much_they_grew() {
    let mut losses = Losses::default();
    let t0 = Duration::from_secs(1);
    assert!(losses.due(t0));
    assert_eq!(losses.read(t0, 0, 0), 0);
    let early = (t0 + QUERY_EVERY)
        .checked_sub(Duration::from_millis(1))
        .unwrap();
    assert!(!losses.due(early));
    assert!(losses.due(t0 + QUERY_EVERY));
    assert_eq!(losses.read(t0 + QUERY_EVERY, 7, 2), 9);
    assert_eq!(losses.read(t0 + QUERY_EVERY * 2, 7, 3), 1);
    // A counter lower than before counts from zero.
    assert_eq!(losses.read(t0 + QUERY_EVERY * 3, 4, 3), 4);
}

#[test]
fn a_key_a_killed_run_left_is_replaced_and_anyone_elses_refused() {
    assert_eq!(key_start(None), KeyStart::Fresh);
    assert_eq!(key_start(Some(true)), KeyStart::Stale);
    assert_eq!(key_start(Some(false)), KeyStart::Foreign);
}

#[test]
fn a_line_with_no_net_reads_as_network() {
    let line = r#"{"v":1,"sid":"S-1-5-21-1-2-3-1003","pid":1,"path":"C:\\x","access":"unknown"}"#;
    assert!(matches!(
        Line::parse(line),
        Some(Line::Denied { net: true, .. })
    ));
}

#[test]
fn a_waiting_create_older_than_two_seconds_is_stale_and_pairs_with_nothing() {
    use super::super::consumer::{STALE_AFTER_SECONDS, STAMP_TICKS_PER_SECOND};
    let mut fed = Fed::new();
    let limit = STALE_AFTER_SECONDS * STAMP_TICKS_PER_SECOND;
    // An end whose own Create came late finds an older, stale entry: dropped, unpaired.
    create_at(&mut fed, 0, 0x60, NAME);
    end_at(&mut fed, limit + 1, 0x60, ACCESS_DENIED);
    assert_eq!(fed.out, []);
    assert_eq!(fed.consumer.pending(), 0);
    // At the limit it still pairs.
    create_at(&mut fed, 10, 0x61, NAME);
    end_at(&mut fed, 10 + limit, 0x61, ACCESS_DENIED);
    assert_eq!(fed.out.len(), 1);
}

#[test]
fn the_stale_window_is_two_seconds_of_100_ns_stamps_whatever_the_perf_freq() {
    // Stamps are system time in 100 ns units (no raw timestamps). Scaled by a 3.579545 MHz
    // `PerfFreq`, the window would be 7,159,090 ticks, 0.7 s, and the end below dropped; it
    // stays 2 s of 100 ns ticks.
    assert_eq!(super::super::consumer::STAMP_TICKS_PER_SECOND, 10_000_000);
    let mut fed = Fed::new();
    create_at(&mut fed, 0, 0x70, NAME);
    end_at(&mut fed, 2 * 3_579_545 + 1, 0x70, ACCESS_DENIED);
    assert_eq!(fed.out.len(), 1, "0.7 s is well inside the window");
    create_at(&mut fed, 0, 0x71, NAME);
    end_at(&mut fed, 20_000_000, 0x71, ACCESS_DENIED);
    assert_eq!(fed.out.len(), 2, "at 2 s it pairs");
    create_at(&mut fed, 0, 0x72, NAME);
    end_at(&mut fed, 20_000_001, 0x72, ACCESS_DENIED);
    assert_eq!(fed.out.len(), 2, "past 2 s it is stale");
}
