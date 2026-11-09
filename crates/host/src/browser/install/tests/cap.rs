//! The download's size cap in process: the poll fails once the zip passes the pin's size, and a
//! failing poll ends the running tool. curl's own `--max-filesize` is not enough before curl
//! 8.4 when the response has no `Content-Length`. The curl download itself is https only, so
//! these test the poll's cut-off and the tool's end directly, not a streaming server.

#[cfg(unix)]
use std::path::Path;

use clauseo_protocol::BrowserInstallFailure;

#[cfg(unix)]
use crate::browser::install::Cancel;
#[cfg(unix)]
use crate::browser::install::system::run_tool;
use crate::browser::install::system::{Poll, Stop};
use crate::pipeline::tests::support::TempDir;
use crate::write;

#[test]
fn the_poll_fails_as_size_mismatch_once_the_zip_passes_the_pin() {
    let dir = TempDir::new("cft-cap-poll");
    let zip = dir.path().join("chrome.zip");
    let headers = dir.path().join("headers.txt");
    let seen = std::cell::RefCell::new(Vec::new());
    let mut progress = |bytes, total| seen.borrow_mut().push((bytes, total));
    let mut poll = Poll {
        headers: &headers,
        zip: &zip,
        max_bytes: 10,
        last: None,
    };
    // No Content-Length (none in the headers): no progress, and no cut-off at or under the cap.
    write::write_file(&headers, b"HTTP/1.1 200 OK\r\n\r\n").unwrap();
    write::write_file(&zip, b"0123456789").unwrap();
    assert!(poll.tick(&mut progress).is_ok());
    // One byte past the pin: stopped, as size_mismatch whatever code the caller maps to.
    write::write_file(&zip, b"0123456789a").unwrap();
    let Err(stop) = poll.tick(&mut progress) else {
        panic!("the poll let the zip pass its pinned size");
    };
    assert!(matches!(stop, Stop::TooLarge(_)));
    let failure = stop.failure(BrowserInstallFailure::Network);
    assert_eq!(failure.code, BrowserInstallFailure::SizeMismatch);
    assert_eq!(*seen.borrow(), Vec::<(u64, u64)>::new());
    // With a Content-Length the progress is reported.
    write::write_file(&headers, b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\n").unwrap();
    write::write_file(&zip, b"01234").unwrap();
    assert!(poll.tick(&mut progress).is_ok());
    assert_eq!(*seen.borrow(), [(5, 10)]);
}

/// A poll that fails ends the tool's tree, as a cancel does: `sleep 30` does not run out.
#[cfg(unix)]
#[test]
fn a_failing_poll_ends_the_running_tool() {
    let dir = TempDir::new("cft-cap-kill");
    let result = run_tool(
        Path::new("/bin/sleep"),
        vec!["30".into()],
        dir.path(),
        &Cancel::new(),
        0,
        &mut || Err(Stop::TooLarge("past the pin".to_owned())),
    );
    assert!(matches!(result, Err(Stop::TooLarge(_))));
}
