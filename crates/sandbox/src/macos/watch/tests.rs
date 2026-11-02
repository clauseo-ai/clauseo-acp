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
fn untagged_duplicate_and_odd_lines_are_ignored() {
    // No tag: another app's denial, or a rule without a message.
    assert_eq!(
        parse(&line("Sandbox: cat(1) deny(1) file-read-data /x")),
        None
    );
    // The log's fold of repeats from one process.
    assert_eq!(
        parse(&line(
            "299 duplicate reports for Sandbox: bash(68116) deny(1) file-write-create /w\nclauseo-1-0"
        )),
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
