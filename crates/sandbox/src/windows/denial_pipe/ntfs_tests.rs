//! The denial pipe on a real machine: a server end made as the launcher makes it (the desktop
//! user alone, one instance), opened by the outer's client end, which no child inherits and
//! which takes the one instance (a second open is busy, and refuses the launch in plain words),
//! records written and read back whole, and a name that isn't a denial pipe's refused before
//! any open.

use std::io::{BufReader, ErrorKind, Write as _};
use std::os::windows::io::AsRawHandle as _;
use std::path::Path;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use windows::Win32::Foundation::HANDLE;

use super::os::{inheritable, open_writer};
use super::sink::open_feed;
use super::{Record, denial_pipe_name, encode, read_record};
use crate::SetAsideWhat;
use crate::windows::pipe::os::ServerPipe;
use crate::windows::pipe::{PIPE_RIGHTS, PipeSet, Role};
use crate::windows::record::current_user_sid;

#[test]
fn a_record_written_to_a_lawyer_only_pipe_reads_back_whole() {
    let name = denial_pipe_name(&PipeSet::random().unwrap());
    let lawyer = current_user_sid().unwrap();
    // As the launcher's: the desktop user alone, the Medium label; inbound, one instance.
    let sddl = format!("D:P(A;;0x{PIPE_RIGHTS:x};;;{lawyer})S:(ML;;NWNR;;;ME)");
    let server = ServerPipe::create(&name, Role::Err, &sddl).unwrap();

    let mut client = open_writer(&name).unwrap();
    assert!(
        !inheritable(&client).unwrap(),
        "no process the outer starts gets it"
    );
    // The one instance is the outer's: another open is busy, and a feed refuses the launch.
    let busy = open_writer(&name).unwrap_err();
    assert_eq!(busy.raw_os_error(), Some(231), "{busy}");
    let Err(said) = open_feed(Some(&name), open_writer) else {
        panic!("a busy pipe let the launch go on");
    };
    assert!(
        said.contains("another program was already connected"),
        "{said}"
    );

    // A thread that runs until the connect returned: its handle is the connect's watch, which
    // never fires meanwhile.
    let (release, held) = mpsc::channel::<()>();
    let runner = thread::spawn(move || held.recv().unwrap_or(()));
    server
        .connect(HANDLE(runner.as_raw_handle()), Duration::from_secs(5))
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

/// A pipe that was never made: the launch is refused, in plain words.
#[test]
fn a_named_pipe_that_is_gone_refuses_the_launch() {
    let name = denial_pipe_name(&PipeSet::random().unwrap());
    let Err(said) = open_feed(Some(&name), open_writer) else {
        panic!("a missing pipe let the launch go on");
    };
    assert!(said.contains("it had already closed"), "{said}");
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
