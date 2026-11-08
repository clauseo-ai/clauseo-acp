//! The watcher's pure parts: parsing, the pool filter, pairing with reused IRPs, the device
//! map, the lines and the rate. The serving loop over a fake [`Watcher`] is in `tests/run.rs`.

use std::time::Duration;

use super::consumer::{
    ACCESS_DENIED, Access, CREATE_ID, Consumer, END_ID, PENDING_CAP, RawEvent, Record, access_of,
    parse_create, parse_end, sid_bytes,
};
use super::paths::{DeviceMap, Mapped, REFRESH_EVERY};
use super::protocol::{
    KEY_SDDL, Line, RECORDS_PER_SECOND, RateLimit, denied_line, dropped_line, is_pipe_name,
    pipe_name, pipe_sddl,
};
use crate::windows::sids::Sid;

mod breaker;
mod consumer;
mod instances;
mod run;

/// Pool users and an outsider (the lawyer), all of one machine.
const POOL_1: &str = "S-1-5-21-1-2-3-1003";
const POOL_2: &str = "S-1-5-21-1-2-3-1004";
const LAWYER: &str = "S-1-5-21-1-2-3-1001";
/// A volume's device, and its letter.
const VOLUME: &str = r"\Device\HarddiskVolume3";

fn sid(text: &str) -> Sid {
    Sid::parse(text).unwrap()
}

fn pool() -> Vec<Sid> {
    vec![sid(POOL_1), sid(POOL_2)]
}

/// A Create's payload with 8-byte pointers.
fn create_data(irp: u64, options: u32, name: &str) -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(&irp.to_le_bytes());
    data.extend_from_slice(&0xffff_8000_0000_1000u64.to_le_bytes());
    data.extend_from_slice(&42u32.to_le_bytes());
    data.extend_from_slice(&options.to_le_bytes());
    data.extend_from_slice(&0x80u32.to_le_bytes());
    data.extend_from_slice(&7u32.to_le_bytes());
    for unit in name.encode_utf16().chain([0]) {
        data.extend_from_slice(&unit.to_le_bytes());
    }
    data
}

/// An `OperationEnd`'s payload with 8-byte pointers.
fn end_data(irp: u64, status: u32) -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(&irp.to_le_bytes());
    data.extend_from_slice(&0u64.to_le_bytes());
    data.extend_from_slice(&status.to_le_bytes());
    data
}

fn devices() -> Vec<(char, String)> {
    vec![
        ('C', VOLUME.to_owned()),
        ('D', r"\Device\HarddiskVolume5".to_owned()),
    ]
}

/// A consumer fed events, collecting what it passes on. Each event is stamped one later than
/// the last unless a test gives its stamp.
struct Fed {
    consumer: Consumer,
    out: Vec<Record>,
    stamp: i64,
}

/// `path` as a local refusal's, as `map_path` gives it.
#[expect(clippy::unnecessary_wraps, reason = "compared with map_path's Option")]
fn local(path: &str) -> Option<Mapped> {
    Some(mapped(path, false))
}

/// `path` as a network refusal's, for display only, as `map_path` gives it.
#[expect(clippy::unnecessary_wraps, reason = "compared with map_path's Option")]
fn net(path: &str) -> Option<Mapped> {
    Some(mapped(path, true))
}

fn mapped(path: &str, net: bool) -> Mapped {
    Mapped {
        path: path.to_owned(),
        net,
    }
}

impl Fed {
    fn new() -> Fed {
        Fed {
            consumer: Consumer::new(&pool()),
            out: Vec::new(),
            stamp: 0,
        }
    }

    fn create(&mut self, who: &str, pid: u32, irp: u64, options: u32, name: &str) {
        let bytes = sid_bytes(&sid(who)).unwrap();
        let data = create_data(irp, options, name);
        self.feed(CREATE_ID, pid, Some(&bytes), &data);
    }

    fn end(&mut self, irp: u64, status: u32) {
        self.feed(END_ID, 4, None, &end_data(irp, status));
    }

    fn feed(&mut self, id: u16, pid: u32, sid: Option<&[u8]>, data: &[u8]) {
        self.stamp += 1;
        let version = u8::from(id == CREATE_ID);
        self.raw(&RawEvent {
            id,
            pid,
            sid,
            data,
            wide: true,
            version,
            stamp: self.stamp,
        });
    }

    fn raw(&mut self, event: &RawEvent<'_>) {
        let out = &mut self.out;
        self.consumer
            .on_event(event, Duration::ZERO, &mut devices, &mut |r| out.push(r));
    }
}

#[test]
fn a_create_and_an_end_read_at_their_offsets() {
    let data = create_data(0xabc, 0x0100_0060, r"\Device\HarddiskVolume3\x");
    let create = parse_create(&data, true).unwrap();
    assert_eq!(create.irp, 0xabc);
    assert_eq!(create.options, 0x0100_0060);
    assert_eq!(create.file_name, r"\Device\HarddiskVolume3\x");
    let end = parse_end(&end_data(0xabc, ACCESS_DENIED), true).unwrap();
    assert_eq!((end.irp, end.status), (0xabc, ACCESS_DENIED));
    // 4-byte pointers: Irp, ExtraInformation, then Status at 8.
    let mut narrow = 9u32.to_le_bytes().to_vec();
    narrow.extend_from_slice(&0u32.to_le_bytes());
    narrow.extend_from_slice(&5u32.to_le_bytes());
    let end = parse_end(&narrow, false).unwrap();
    assert_eq!((end.irp, end.status), (9, 5));
    // Too short, or a name with no NUL, is nothing.
    assert_eq!(parse_end(&end_data(1, 0)[..19], true), None);
    let mut cut = create_data(1, 0, "abc");
    cut.truncate(cut.len() - 2);
    assert_eq!(parse_create(&cut, true), None);
    assert_eq!(parse_create(&cut[..20], true), None);
}

#[test]
fn a_sid_in_binary_is_what_windows_writes() {
    let bytes = sid_bytes(&sid("S-1-5-21-1-2-3-1003")).unwrap();
    let mut expected = vec![1u8, 5, 0, 0, 0, 0, 0, 5];
    for sub in [21u32, 1, 2, 3, 1003] {
        expected.extend_from_slice(&sub.to_le_bytes());
    }
    assert_eq!(bytes, expected);
    assert_eq!(sid_bytes(&sid("S-1-5-18")).unwrap().len(), 12);
}

#[test]
fn a_pool_users_refused_open_is_passed_on_with_its_drive_letter() {
    let mut fed = Fed::new();
    let name = r"\Device\HarddiskVolume3\Users\lawyer\Documents\brief.docx";
    fed.create(POOL_1, 1234, 0x10, 0x0100_0000, name);
    fed.end(0x10, ACCESS_DENIED);
    assert_eq!(
        fed.out,
        vec![Record {
            sid: sid(POOL_1),
            pid: 1234,
            path: r"C:\Users\lawyer\Documents\brief.docx".to_owned(),
            net: false,
            access: Access::Unknown,
        }]
    );
    // An open that succeeded is nothing; so is an end with no Create waiting.
    fed.create(POOL_2, 9, 0x20, 0x0100_0000, name);
    fed.end(0x20, 0);
    fed.end(0x30, ACCESS_DENIED);
    assert_eq!(fed.out.len(), 1);
    assert_eq!(fed.consumer.pending(), 0);
}

#[test]
fn a_reused_irp_pairs_each_create_with_the_next_end() {
    let mut fed = Fed::new();
    let first = r"\Device\HarddiskVolume3\first";
    let second = r"\Device\HarddiskVolume3\second";
    fed.create(POOL_1, 1, 0x10, 0x0100_0000, first);
    fed.end(0x10, 0);
    fed.create(POOL_1, 1, 0x10, 0x0100_0000, second);
    fed.end(0x10, ACCESS_DENIED);
    let paths: Vec<_> = fed.out.iter().map(|r| r.path.as_str()).collect();
    assert_eq!(paths, [r"C:\second"]);
    // A Create whose end was lost is replaced by the next of its IRP.
    fed.create(POOL_1, 1, 0x40, 0x0100_0000, first);
    fed.create(POOL_2, 2, 0x40, 0x0100_0000, second);
    fed.end(0x40, ACCESS_DENIED);
    assert_eq!(
        fed.out.last().map(|r| (r.pid, r.path.as_str())),
        Some((2, r"C:\second"))
    );
    // The same IRP's next end, with no Create since, is nothing.
    fed.end(0x40, ACCESS_DENIED);
    assert_eq!(fed.out.len(), 2);
}

#[test]
fn a_non_pool_create_is_dropped_and_never_reaches_the_output() {
    let mut fed = Fed::new();
    let lawyer = sid_bytes(&sid(LAWYER)).unwrap();
    let data = create_data(0x10, 0x0100_0000, r"\Device\HarddiskVolume3\private.txt");
    fed.feed(CREATE_ID, 77, Some(&lawyer), &data);
    fed.feed(
        CREATE_ID,
        78,
        None,
        &create_data(0x11, 0, r"\Device\HarddiskVolume3\y"),
    );
    fed.end(0x10, ACCESS_DENIED);
    fed.end(0x11, ACCESS_DENIED);
    assert_eq!(fed.out, []);
    assert_eq!(fed.consumer.pending(), 0);
    assert_eq!(fed.consumer.counts().kept, 0);
    // A non-pool Create that reuses a pool Create's IRP takes it away: the next end is the
    // non-pool open's, so it can't be reported as the pool user's.
    fed.create(
        POOL_1,
        1,
        0x50,
        0x0100_0000,
        r"\Device\HarddiskVolume3\mine",
    );
    fed.feed(
        CREATE_ID,
        77,
        Some(&lawyer),
        &create_data(0x50, 0, r"\Device\X\theirs"),
    );
    fed.end(0x50, ACCESS_DENIED);
    assert_eq!(fed.out, []);
}

#[test]
fn the_pending_map_is_capped_and_evicts_the_oldest() {
    let mut fed = Fed::new();
    let name = r"\Device\HarddiskVolume3\f";
    for irp in 0..(PENDING_CAP as u64 + 10) {
        fed.create(POOL_1, 1, irp, 0x0100_0000, name);
    }
    assert_eq!(fed.consumer.pending(), PENDING_CAP);
    fed.end(0, ACCESS_DENIED);
    assert_eq!(fed.out, []);
    fed.end(PENDING_CAP as u64 + 9, ACCESS_DENIED);
    assert_eq!(fed.out.len(), 1);
    // Replacing one IRP over and over keeps the map at one entry.
    let mut fed = Fed::new();
    for _ in 0..(3 * PENDING_CAP) {
        fed.create(POOL_1, 1, 7, 0x0100_0000, name);
    }
    assert_eq!(fed.consumer.pending(), 1);
    fed.end(7, ACCESS_DENIED);
    assert_eq!(fed.out.len(), 1);
}

#[test]
fn the_disposition_and_delete_on_close_say_write() {
    assert_eq!(access_of(0x0100_0000), Access::Unknown); // FILE_OPEN
    assert_eq!(access_of(0x0100_1000), Access::Write); // FILE_OPEN, delete on close
    for disposition in [0u32, 2, 3, 4, 5] {
        assert_eq!(access_of(disposition << 24), Access::Write, "{disposition}");
    }
    assert_eq!(access_of(0x0900_0000), Access::Unknown);
}

#[test]
fn device_paths_map_to_drive_letters_and_shares_and_nothing_else() {
    let mut map = DeviceMap::default();
    let mut listed = 0;
    let mut list = || {
        listed += 1;
        devices()
    };
    let at = Duration::from_secs(10);
    let to = |map: &mut DeviceMap, path: &str, list: &mut dyn FnMut() -> Vec<(char, String)>| {
        map.map_path(path, at, list)
    };
    assert_eq!(
        to(&mut map, r"\Device\HarddiskVolume3\Users\a.txt", &mut list),
        local(r"C:\Users\a.txt")
    );
    assert_eq!(
        to(&mut map, r"\device\harddiskvolume5\x", &mut list),
        local(r"D:\x")
    );
    assert_eq!(to(&mut map, VOLUME, &mut list), local(r"C:\"));
    // Volume 30 isn't volume 3: no known local volume, so flagged.
    let thirty = r"\Device\HarddiskVolume30\x";
    assert_eq!(to(&mut map, thirty, &mut list), net(thirty));
    assert_eq!(
        to(&mut map, r"\Device\NamedPipe\x", &mut list),
        net(r"\Device\NamedPipe\x")
    );
    // Not a device path at all: dropped.
    assert_eq!(to(&mut map, r"C:\x", &mut list), None);
    assert_eq!(
        listed, 1,
        "the misses within a second list the letters once"
    );
}

#[test]
fn a_miss_lists_the_letters_again_once_a_second() {
    let mut map = DeviceMap::default();
    let mut drives = vec![('C', VOLUME.to_owned())];
    let path = r"\Device\HarddiskVolume9\new";
    let list = |d: &Vec<(char, String)>| d.clone();
    let t0 = Duration::from_secs(5);
    assert_eq!(map.map_path(path, t0, &mut || list(&drives)), net(path));
    // A drive mounted since: not seen until a second has passed.
    drives.push(('E', r"\Device\HarddiskVolume9".to_owned()));
    let soon = t0 + Duration::from_millis(500);
    assert_eq!(map.map_path(path, soon, &mut || list(&drives)), net(path));
    let later = t0 + REFRESH_EVERY;
    assert_eq!(
        map.map_path(path, later, &mut || list(&drives)),
        local(r"E:\new")
    );
    // A letter with no device isn't a prefix of everything.
    let mut map = DeviceMap::default();
    let mut odd = || vec![('Z', String::new()), ('1', VOLUME.to_owned())];
    assert_eq!(map.map_path(path, t0, &mut odd), net(path));
}

#[test]
fn lines_are_one_json_object_each_and_parse_back() {
    let record = Record {
        sid: sid(POOL_1),
        pid: 1234,
        path: r"C:\Users\a b\c.txt".to_owned(),
        net: false,
        access: Access::Write,
    };
    let line = denied_line(&record, &pool()).unwrap();
    assert_eq!(
        line,
        "{\"v\":1,\"sid\":\"S-1-5-21-1-2-3-1003\",\"pid\":1234,\
         \"path\":\"C:\\\\Users\\\\a b\\\\c.txt\",\"access\":\"write\"}\n"
    );
    assert_eq!(
        Line::parse(line.trim_end()),
        Some(Line::Denied {
            v: 1,
            sid: POOL_1.to_owned(),
            pid: 1234,
            path: record.path.clone(),
            access: "write".to_owned(),
            net: false,
        })
    );
    assert_eq!(dropped_line(7), "{\"v\":1,\"dropped\":7}\n");
    assert_eq!(
        Line::parse("{\"v\":1,\"dropped\":7}"),
        Some(Line::Dropped { v: 1, dropped: 7 })
    );
    assert_eq!(Line::parse("{\"v\":2,\"dropped\":7}"), None);
    // The writer's own check: a record not of the pool makes no line.
    let theirs = Record {
        sid: sid(LAWYER),
        ..record
    };
    assert_eq!(denied_line(&theirs, &pool()), None);
}

#[test]
fn the_pipe_name_and_descriptors_are_as_the_protocol_says() {
    let name = pipe_name(&[0xab; 16]);
    assert_eq!(
        name,
        format!(r"\\.\pipe\clauseo-sandbox-watch-{}", "ab".repeat(16))
    );
    assert!(is_pipe_name(&name));
    for other in [
        r"\\.\pipe\clauseo-sandbox-watch-AB",
        r"\\.\pipe\clauseo-run-abababababababababababababababab",
        r"\\server\pipe\clauseo-sandbox-watch-abababababababababababababababab",
    ] {
        assert!(!is_pipe_name(other), "{other}");
    }
    assert_eq!(
        pipe_sddl(&sid(LAWYER)),
        "O:SYD:P(A;;FA;;;SY)(A;;FR;;;S-1-5-21-1-2-3-1001)S:(ML;;NWNR;;;ME)"
    );
    assert!(KEY_SDDL.starts_with("O:SYD:P(A;;KA;;;SY)"));
    assert!(!KEY_SDDL.contains("KA;;;BU") && !KEY_SDDL.contains("KW"));
}

#[test]
fn the_rate_admits_its_limit_a_second_and_counts_the_rest() {
    let t0 = Duration::from_secs(1);
    let mut rate = RateLimit::new(t0);
    let admitted = (0..RECORDS_PER_SECOND + 50)
        .filter(|_| rate.admit(t0))
        .count();
    assert_eq!(admitted, RECORDS_PER_SECOND as usize);
    rate.lose(5);
    assert_eq!(rate.roll(t0 + Duration::from_millis(999)), None);
    assert_eq!(rate.roll(t0 + Duration::from_secs(1)), Some(55));
    // The next window admits again, and with nothing dropped there is no line.
    assert!(rate.admit(t0 + Duration::from_millis(1100)));
    assert_eq!(rate.roll(t0 + Duration::from_secs(3)), None);
}
