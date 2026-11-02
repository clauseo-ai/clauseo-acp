use std::sync::Mutex;

use super::*;

/// A line as `log stream --style ndjson` writes it (the spike's sample, cut to the fields the
/// watcher reads and a few around them), with `message` as its eventMessage.
fn line(message: &str) -> String {
    serde_json::json!({
        "messageType": "Error",
        "eventType": "logEvent",
        "processImagePath": "/kernel",
        "senderImagePath": "/System/Library/Extensions/Sandbox.kext/Contents/MacOS/Sandbox",
        "processID": 0,
        "timestamp": "2026-11-02 23:17:14.568427+0530",
        "eventMessage": message,
    })
    .to_string()
}

#[test]
fn a_read_denial_parses() {
    let found = parse(&line(
        "Sandbox: cat(44443) deny(1) file-read-data /private/tmp/s/secret.txt\nclauseo-12-0",
    ))
    .unwrap();
    assert_eq!(
        found,
        LogDenial {
            tag: "clauseo-12-0".to_owned(),
            repeats: 0,
            process: "cat".to_owned(),
            pid: 44443,
            operation: "file-read-data".to_owned(),
            target: "/private/tmp/s/secret.txt".to_owned(),
        }
    );
    assert_eq!(access(&found.operation), Some(Access::Read));
}

#[test]
fn the_spike_samples_parse() {
    // Verbatim eventMessages from the spike (`/tmp/stagec/spike/report.md`).
    let write = parse(&line(
        "Sandbox: bash(44447) deny(1) file-write-create /private/tmp/stagec/spike/w.txt\nclauseo-spike-w",
    ))
    .unwrap();
    assert_eq!(write.operation, "file-write-create");
    assert_eq!(access(&write.operation), Some(Access::Write));
    let net = parse(&line(
        "Sandbox: nc(44451) deny(1) network-outbound remote:*:443\nclauseo-spike-n",
    ))
    .unwrap();
    assert_eq!(net.target, "remote:*:443");
    assert_eq!(access(&net.operation), None);
}

#[test]
fn a_path_with_spaces_newlines_and_a_tag_keeps_the_last_line_as_the_tag() {
    let found = parse(&line(
        "Sandbox: my tool(7) deny(1) file-read-data /m/a b\nclauseo-9-9\nc.txt\nclauseo-1-2",
    ))
    .unwrap();
    assert_eq!(found.tag, "clauseo-1-2");
    assert_eq!(found.process, "my tool");
    assert_eq!(found.target, "/m/a b\nclauseo-9-9\nc.txt");
}

#[test]
fn untagged_and_odd_lines_are_ignored() {
    // No tag: another app's denial, or a rule without a message.
    assert_eq!(
        parse(&line("Sandbox: cat(1) deny(1) file-read-data /x")),
        None
    );
    // A tag of the wrong shape, the header line, and JSON without a message.
    assert_eq!(
        parse(&line(
            "Sandbox: cat(1) deny(1) file-read-data /x\nclauseo-\"x"
        )),
        None
    );
    assert_eq!(
        parse("Filtering the log data using \"sender == ...\""),
        None
    );
    assert_eq!(parse("{\"eventMessage\": 3}"), None);
    assert_eq!(
        parse(&line(
            "Sandbox: cat(x) deny(1) file-read-data /x\nclauseo-1-0"
        )),
        None
    );
}

#[test]
fn only_data_reads_and_writes_are_reported() {
    assert_eq!(access("file-read-data"), Some(Access::Read));
    for op in [
        "file-write-data",
        "file-write-unlink",
        "file-write-create",
        "file-write-mode",
    ] {
        assert_eq!(access(op), Some(Access::Write), "{op}");
    }
    for op in [
        "file-read-metadata",
        "file-read-xattr",
        "file-test-existence",
        "process-exec",
        "mach-lookup",
        "network-outbound",
    ] {
        assert_eq!(access(op), None, "{op}");
    }
}

/// A sink that keeps what it gets.
#[derive(Default)]
struct Keep(Mutex<Vec<Denial>>);

impl DenialSink for Keep {
    fn denied(&self, d: Denial) {
        self.0.lock().unwrap().push(d);
    }
}

#[test]
fn the_logs_fold_of_repeats_reaches_the_sink_as_dropped_and_devices_do_not() {
    let folded = parse(&line(
        "299 duplicate reports for Sandbox: bash(68116) deny(1) file-write-create /w\nclauseo-1-0",
    ))
    .unwrap();
    assert_eq!(folded.repeats, 299);
    assert_eq!(folded.operation, "file-write-create");
    assert_eq!(folded.target, "/w");
    let one = parse(&line(
        "1 duplicate report for Sandbox: cat(5) deny(1) file-read-data /r\nclauseo-1-0",
    ))
    .unwrap();
    assert_eq!(one.repeats, 1);
    // A path holding the marker is a plain report.
    let plain = parse(&line(
        "Sandbox: cat(5) deny(1) file-read-data /a 2 duplicate reports for b\nclauseo-1-0",
    ))
    .unwrap();
    assert_eq!(plain.repeats, 0);
    assert_eq!(plain.target, "/a 2 duplicate reports for b");

    let dir = crate::test_support::TempDir::new("watch-fold");
    dir.dir("m");
    dir.dir("s");
    let policy = Policy::builder(dir.path().join("m"), dir.path().join("s"))
        .home(dir.path().join("home"))
        .build()
        .unwrap();
    let keep = Arc::new(Keep::default());
    let inner: Arc<dyn DenialSink> = Arc::clone(&keep) as Arc<dyn DenialSink>;
    let sink: LaunchSink = crate::CappedSink::new(inner);
    report(&folded, &policy, &sink);
    // A fold of denials the watcher does not report (metadata) passes nothing on.
    let metadata = LogDenial {
        operation: "file-read-metadata".to_owned(),
        ..folded
    };
    report(&metadata, &policy, &sink);
    // A device is never reported: every shell asks for /dev/tty.
    let tty = parse(&line(
        "Sandbox: sh(5) deny(1) file-read-data /dev/tty\nclauseo-1-0",
    ))
    .unwrap();
    report(&tty, &policy, &sink);
    assert_eq!(*keep.0.lock().unwrap(), [Denial::Dropped { count: 299 }]);
}
