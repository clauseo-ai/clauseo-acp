//! The denial pipe on a real machine: made for the lawyer alone, one instance, opened by the
//! outer's client end, records written and read back whole, the client's pid seen by the
//! server, and a name that isn't a denial pipe's refused before any open.

use std::io::{BufReader, ErrorKind, Write as _};
use std::os::windows::io::AsRawHandle as _;
use std::path::Path;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use super::os::{DenialServer, open_writer};
use super::{Record, encode, random_pipe_name, read_record};
use crate::SetAsideWhat;
use crate::windows::record::current_user_sid;

#[test]
fn a_record_written_to_a_lawyer_only_pipe_reads_back_whole() {
    let name = random_pipe_name().unwrap();
    let lawyer = current_user_sid().unwrap();
    let server = DenialServer::create(&name, &lawyer).unwrap();
    // One instance, made first: a second server on the name is refused.
    assert!(DenialServer::create(&name, &lawyer).is_err());

    let mut client = open_writer(&name).unwrap();
    // A thread that runs until the connect returned: its handle is the connect's watch, which
    // never fires meanwhile.
    let (release, held) = mpsc::channel::<()>();
    let runner = thread::spawn(move || held.recv().unwrap_or(()));
    server
        .connect(runner.as_raw_handle(), Duration::from_secs(5))
        .unwrap();
    release.send(()).unwrap();
    runner.join().unwrap();
    assert_eq!(server.client_pid().unwrap(), std::process::id());

    let record = Record::set_aside(
        Path::new(r"Precedents\.git"),
        Path::new(r"C:\aside\.git"),
        SetAsideWhat::GitRepository,
    );
    let notice = Record::notice("Clauseo stopped it.");
    client.write_all(&encode(&record).unwrap()).unwrap();
    client.write_all(&encode(&notice).unwrap()).unwrap();
    drop(client);

    let mut reader = BufReader::new(&server);
    assert_eq!(read_record(&mut reader).unwrap(), Some(record));
    assert_eq!(read_record(&mut reader).unwrap(), Some(notice));
    assert_eq!(read_record(&mut reader).unwrap(), None);
}

#[test]
fn a_name_that_isnt_a_denial_pipe_is_refused_before_any_open() {
    for name in [
        r"\\.\pipe\clauseo-run-x-err",
        r"C:\Windows\win.ini",
        r"\\server\pipe\x",
    ] {
        let error = open_writer(name).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::InvalidInput, "{name}");
    }
}
