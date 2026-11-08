//! The denial pipe's names, descriptor and records, and the outer's sink, on every OS: a round
//! trip, the strict decode's refusals, and a stderr line in the old shape read as nothing.

use std::io::{self, BufReader, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::sink::{RecordSink, Sent};
use super::{
    DENIAL_PIPE_PREFIX, NotADenialPipe, RECORD_CAP, Record, RecordError, check_pipe_name, decode,
    denial_pipe_sddl, encode, random_pipe_name, read_record,
};
use crate::windows::sids::Sid;
use crate::{Denial, SetAsideWhat};

fn moved() -> Record {
    Record::set_aside(
        Path::new(r"Precedents\.git"),
        Path::new(r"C:\Users\lawyer\Clauseo\aside\.git"),
        SetAsideWhat::GitRepository,
    )
}

#[test]
fn a_record_round_trips_as_one_versioned_json_line() {
    for record in [
        moved(),
        Record::notice("Clauseo stopped this session \"now\"\n."),
    ] {
        let line = encode(&record).unwrap();
        assert_eq!(line.last(), Some(&b'\n'));
        assert_eq!(line.iter().position(|b| *b == b'\n'), Some(line.len() - 1));
        assert!(line.starts_with(br#"{"v":1,"record":{"kind":"#));
        assert_eq!(decode(&line).unwrap(), record);
    }
    assert_eq!(
        String::from_utf8(encode(&moved()).unwrap()).unwrap(),
        "{\"v\":1,\"record\":{\"kind\":\"set_aside\",\"path\":\"Precedents\\\\.git\",\"to\":\
         \"C:\\\\Users\\\\lawyer\\\\Clauseo\\\\aside\\\\.git\",\"what\":\"git_repository\"}}\n"
    );
    assert_eq!(
        moved().denial(),
        Some(Denial::SetAside {
            path: r"Precedents\.git".into(),
            to: r"C:\Users\lawyer\Clauseo\aside\.git".into(),
            what: SetAsideWhat::GitRepository,
        })
    );
    assert_eq!(Record::notice("x").denial(), None);
}

#[test]
fn the_decode_refuses_other_versions_unknown_shapes_garbage_and_oversize() {
    let good = r#"{"v":1,"record":{"kind":"notice","text":"hi"}}"#;
    assert_eq!(decode(good.as_bytes()).unwrap(), Record::notice("hi"));
    assert_eq!(
        decode(br#"{"v":2,"record":{"kind":"notice","text":"hi"}}"#),
        Err(RecordError::Version(2))
    );
    for bad in [
        r#"{"record":{"kind":"notice","text":"hi"}}"#,
        r#"{"v":1,"record":{"kind":"notice","text":"hi"},"extra":1}"#,
        r#"{"v":1,"record":{"kind":"notice","text":"hi","extra":1}}"#,
        r#"{"v":1,"record":{"kind":"denial","text":"hi"}}"#,
        r#"{"v":1,"record":{"kind":"set_aside","path":"a","to":"b","what":"anything"}}"#,
        r#"{"v":1,"record":{"kind":"set_aside","path":"a","to":"b"}}"#,
        r#"{"v":1,"record":{"kind":"notice","text":"hi"}} {"v":1}"#,
        "",
        "\u{0}\u{1}garbage",
    ] {
        assert!(
            matches!(decode(bad.as_bytes()), Err(RecordError::Malformed(_))),
            "{bad}"
        );
    }
    let long = Record::notice("x".repeat(RECORD_CAP));
    assert_eq!(encode(&long), Err(RecordError::TooLong));
    let mut huge = br#"{"v":1,"record":{"kind":"notice","text":""#.to_vec();
    huge.resize(RECORD_CAP + 1, b'x');
    assert_eq!(decode(&huge), Err(RecordError::TooLong));
    // The error never quotes what it read.
    let quoted = decode(br#"{"v":1,"record":{"kind":"tok-SECRET"}}"#).unwrap_err();
    assert!(!quoted.to_string().contains("SECRET"), "{quoted}");
}

#[test]
fn records_are_read_one_line_at_a_time_up_to_the_cap() {
    let mut stream = encode(&moved()).unwrap();
    stream.extend(encode(&Record::notice("two")).unwrap());
    let mut reader = BufReader::new(stream.as_slice());
    assert_eq!(read_record(&mut reader).unwrap(), Some(moved()));
    assert_eq!(
        read_record(&mut reader).unwrap(),
        Some(Record::notice("two"))
    );
    assert_eq!(read_record(&mut reader).unwrap(), None);

    let cut = &encode(&moved()).unwrap()[..20];
    assert_eq!(
        read_record(&mut BufReader::new(cut)),
        Err(RecordError::Truncated)
    );
    let endless = vec![b'x'; RECORD_CAP * 2];
    assert_eq!(
        read_record(&mut BufReader::new(endless.as_slice())),
        Err(RecordError::TooLong)
    );
}

/// The old stderr line, as an agent could write it to its own stderr (which the outer relays),
/// and a notice in the outer's old words: neither is a record, and a record is the one way a
/// set-aside or a notice reaches the host now.
#[test]
fn a_forged_stderr_line_makes_no_denial_and_no_notice() {
    let forged = [
        format!(
            "clauseo-sandbox-run: set aside\tfoo\taside\\foo\t{}",
            SetAsideWhat::LinkOut.why()
        ),
        "clauseo-sandbox-run: Clauseo stopped this session because the AI tool tried.".to_owned(),
    ];
    for line in forged {
        assert!(decode(line.as_bytes()).is_err(), "{line}");
        let stream = format!("{line}\n");
        assert!(read_record(&mut BufReader::new(stream.as_bytes())).is_err());
    }
}

#[test]
fn a_pipe_name_is_the_local_root_the_prefix_and_32_hex() {
    let name = random_pipe_name().unwrap();
    assert!(name.starts_with(&format!(r"\\.\pipe\{DENIAL_PIPE_PREFIX}")));
    assert_eq!(check_pipe_name(&name), Ok(()));
    assert_ne!(name, random_pipe_name().unwrap());
    let hex = "0123456789abcdef0123456789abcdef";
    let short = "123456789abcdef0123456789abcdef";
    assert_eq!(
        check_pipe_name(&format!(r"\\.\pipe\clauseo-denials-{hex}")),
        Ok(())
    );
    for bad in [
        format!(r"\\server\pipe\clauseo-denials-{hex}"),
        format!(r"\\?\pipe\clauseo-denials-{hex}"),
        format!(r"\\.\PIPE\clauseo-denials-{hex}"),
        format!(r"\\.\pipe\clauseo-run-{hex}-err"),
        format!(r"\\.\pipe\clauseo-denials-{}", hex.to_uppercase()),
        format!(r"\\.\pipe\clauseo-denials-{hex}0"),
        format!(r"\\.\pipe\clauseo-denials-{short}"),
        format!(r"\\.\pipe\clauseo-denials-{hex}\..\x"),
        format!(r"\\.\pipe\x\..\clauseo-denials-{hex}"),
        format!(r"C:\clauseo-denials-{hex}"),
        format!("clauseo-denials-{hex}"),
        format!("\\\\.\\pipe\\clauseo-denials-{short}\0"),
        String::new(),
    ] {
        assert_eq!(check_pipe_name(&bad), Err(NotADenialPipe), "{bad:?}");
    }
}

#[test]
fn the_pipes_descriptor_names_the_lawyer_alone() {
    let lawyer = Sid::parse("S-1-5-21-1-2-3-1001").unwrap();
    assert_eq!(
        denial_pipe_sddl(&lawyer),
        "D:P(A;;0x12019b;;;S-1-5-21-1-2-3-1001)S:(ML;;NWNR;;;ME)"
    );
}

/// A writer the test reads back; it fails every write once `broken`.
#[derive(Clone, Default)]
struct Pipe {
    bytes: Arc<Mutex<Vec<u8>>>,
    broken: bool,
}

impl Write for Pipe {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.broken {
            return Err(io::Error::from(io::ErrorKind::BrokenPipe));
        }
        self.bytes.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn the_sink_writes_each_record_as_a_line_and_drains_at_the_end() {
    let pipe = Pipe::default();
    let sink = RecordSink::start(Some(Box::new(pipe.clone())));
    assert_eq!(sink.send(&Record::notice("one")), Sent::Queued);
    assert_eq!(sink.send(&moved()), Sent::Queued);
    sink.drain(Duration::from_secs(5));
    let bytes = pipe.bytes.lock().unwrap().clone();
    let mut reader = BufReader::new(bytes.as_slice());
    assert_eq!(
        read_record(&mut reader).unwrap(),
        Some(Record::notice("one"))
    );
    assert_eq!(read_record(&mut reader).unwrap(), Some(moved()));
    assert_eq!(read_record(&mut reader).unwrap(), None);
    // Drained: what comes later is only logged.
    assert_eq!(sink.send(&moved()), Sent::Logged);
}

#[test]
fn with_no_pipe_or_a_broken_one_records_are_only_logged() {
    let none = RecordSink::start(None);
    assert_eq!(none.send(&moved()), Sent::Logged);
    none.drain(Duration::from_secs(1));

    let broken = Pipe {
        broken: true,
        ..Pipe::default()
    };
    let sink = RecordSink::start(Some(Box::new(broken.clone())));
    let _first = sink.send(&moved());
    sink.drain(Duration::from_secs(5));
    assert_eq!(sink.send(&moved()), Sent::Logged);
    assert!(broken.bytes.lock().unwrap().is_empty());
    // A record past the cap is never queued.
    let sink = RecordSink::start(Some(Box::new(Pipe::default())));
    assert_eq!(
        sink.send(&Record::notice("x".repeat(RECORD_CAP))),
        Sent::Logged
    );
}
