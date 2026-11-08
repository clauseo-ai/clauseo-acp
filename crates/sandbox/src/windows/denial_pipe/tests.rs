//! The denial pipe's names and records, and the outer's sink, on every OS: a round trip, the
//! strict decode's refusals, a stderr line in the old shape read as nothing, and the feed that
//! refuses the launch when the pipe it names can't be opened.

use std::io::{self, BufReader, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::sink::{RecordSink, Sent, open_feed};
use super::{
    NotADenialPipe, RECORD_CAP, Record, RecordError, check_pipe_name, decode, denial_pipe_name,
    encode, read_record,
};
use crate::windows::pipe::PipeSet;
use crate::{Denial, SetAsideWhat};

fn moved() -> Record {
    Record::set_aside(
        Path::new(r"Precedents\.git"),
        Path::new(r"C:\Users\lawyer\Clauseo\aside\.git"),
        SetAsideWhat::GitRepository,
    )
}

#[test]
fn a_record_round_trips_as_one_kind_tagged_json_line() {
    for record in [
        moved(),
        Record::notice("Clauseo stopped this session \"now\"\n."),
    ] {
        let line = encode(&record).unwrap();
        assert_eq!(line.last(), Some(&b'\n'));
        assert_eq!(line.iter().position(|b| *b == b'\n'), Some(line.len() - 1));
        assert!(line.starts_with(br#"{"kind":"#));
        assert_eq!(decode(&line).unwrap(), record);
    }
    assert_eq!(
        String::from_utf8(encode(&moved()).unwrap()).unwrap(),
        "{\"kind\":\"set_aside\",\"path\":\"Precedents\\\\.git\",\"to\":\
         \"C:\\\\Users\\\\lawyer\\\\Clauseo\\\\aside\\\\.git\",\"what\":\"git_repository\"}\n"
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

/// The launcher's stand-in reader (PR6 C4, `launch/records.rs`) writes the same set-aside line.
#[test]
fn the_launchers_set_aside_line_reads_back() {
    let line = br#"{"kind":"set_aside","path":"a\\.git","to":"C:\\x\\.git","what":"link_out"}"#;
    assert_eq!(
        decode(line).unwrap(),
        Record::SetAside {
            path: r"a\.git".to_owned(),
            to: r"C:\x\.git".to_owned(),
            what: SetAsideWhat::LinkOut,
        }
    );
}

#[test]
fn the_decode_refuses_unknown_shapes_garbage_and_oversize() {
    let good = r#"{"kind":"notice","text":"hi"}"#;
    assert_eq!(decode(good.as_bytes()).unwrap(), Record::notice("hi"));
    for bad in [
        r#"{"v":1,"kind":"notice","text":"hi"}"#,
        r#"{"v":1,"record":{"kind":"notice","text":"hi"}}"#,
        r#"{"kind":"notice","text":"hi","extra":1}"#,
        r#"{"kind":"denial","text":"hi"}"#,
        r#"{"text":"hi"}"#,
        r#"{"kind":"set_aside","path":"a","to":"b","what":"anything"}"#,
        r#"{"kind":"set_aside","path":"a","to":"b"}"#,
        r#"{"kind":"notice","text":"hi"} {"kind":"notice","text":"hi"}"#,
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
    let mut huge = br#"{"kind":"notice","text":""#.to_vec();
    huge.resize(RECORD_CAP + 1, b'x');
    assert_eq!(decode(&huge), Err(RecordError::TooLong));
    // The error never quotes what it read.
    let quoted = decode(br#"{"kind":"tok-SECRET"}"#).unwrap_err();
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
fn a_pipe_name_is_a_runner_pipe_sets_denials_pipe() {
    let set = PipeSet::random().unwrap();
    let name = denial_pipe_name(&set);
    assert_eq!(name, format!(r"\\.\pipe\{}-denials", set.base()));
    assert_eq!(check_pipe_name(&name), Ok(()));
    let hex = "0123456789abcdef0123456789abcdef";
    let short = "123456789abcdef0123456789abcdef";
    assert_eq!(
        check_pipe_name(&format!(r"\\.\pipe\clauseo-run-{hex}-denials")),
        Ok(())
    );
    for bad in [
        format!(r"\\server\pipe\clauseo-run-{hex}-denials"),
        format!(r"\\?\pipe\clauseo-run-{hex}-denials"),
        format!(r"\\.\PIPE\clauseo-run-{hex}-denials"),
        format!(r"\\.\pipe\clauseo-run-{hex}-err"),
        format!(r"\\.\pipe\clauseo-run-{hex}-ctl"),
        format!(r"\\.\pipe\clauseo-run-{hex}"),
        format!(r"\\.\pipe\clauseo-denials-{hex}"),
        format!(r"\\.\pipe\clauseo-run-{}-denials", hex.to_uppercase()),
        format!(r"\\.\pipe\clauseo-run-{hex}0-denials"),
        format!(r"\\.\pipe\clauseo-run-{short}-denials"),
        format!(r"\\.\pipe\clauseo-run-{hex}-denials\..\x"),
        format!(r"\\.\pipe\x\..\clauseo-run-{hex}-denials"),
        format!(r"C:\clauseo-run-{hex}-denials"),
        format!("clauseo-run-{hex}-denials"),
        format!("\\\\.\\pipe\\clauseo-run-{hex}-denials\0"),
        String::new(),
    ] {
        assert_eq!(check_pipe_name(&bad), Err(NotADenialPipe), "{bad:?}");
    }
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

/// No pipe named: a sink that only logs, and nothing is opened.
#[test]
fn with_no_pipe_named_the_feed_only_logs() {
    let sink = open_feed(None, |_name| -> io::Result<Pipe> {
        panic!("nothing is opened")
    })
    .unwrap();
    assert_eq!(sink.send(&moved()), Sent::Logged);
}

/// A pipe named but taken, gone or unreachable refuses the launch, in plain words; one that
/// opens takes the records.
#[test]
fn a_named_pipe_that_cant_be_opened_refuses_the_launch_in_plain_words() {
    let name = denial_pipe_name(&PipeSet::random().unwrap());
    for (code, why) in [
        (231, "another program was already connected to it"),
        (2, "it had already closed"),
        (53, "it couldn't be reached (error 53)"),
    ] {
        let Err(said) = open_feed(Some(&name), |opened| -> io::Result<Pipe> {
            assert_eq!(opened, name);
            Err(io::Error::from_raw_os_error(code))
        }) else {
            panic!("{code}: the launch went on");
        };
        assert!(
            said.starts_with("Clauseo didn't start the session"),
            "{said}"
        );
        assert!(said.contains(why), "{said}");
        assert!(!said.contains(&name), "{said}");
    }
    let pipe = Pipe::default();
    let sink = open_feed(Some(&name), |_name| Ok(pipe.clone())).unwrap();
    assert_eq!(sink.send(&moved()), Sent::Queued);
    sink.drain(Duration::from_secs(5));
    assert!(!pipe.bytes.lock().unwrap().is_empty());
}
