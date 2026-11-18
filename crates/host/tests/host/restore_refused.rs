//! A restore the folder refuses one file of: the rest is restored, and the answer is
//! `matter.file_refused` naming the file.

use clauseo_protocol::{
    CheckpointKind, CommandOutcome, DomainEvent, ErrorCode, HostCommand, MatterPath, RestoreRequest,
};

use crate::support::{Dir, FAKE, Scenario, matter_with_thread};
use crate::versions::matter_events;
use crate::write;

/// A restore the folder refuses one file of still restores the rest, takes its checkpoint and
/// logs `VersionRestored`, then answers `matter.file_refused`: the refused file in
/// `HostError.files`, its cause and the restore's file count in `HostError.refusal`, no path in
/// the message. The refused file is as it was, and a second restore puts it back too.
#[test]
fn a_restore_with_a_refused_file_restores_the_rest_and_answers_file_refused() {
    let dir = Dir::new(
        "a_restore_with_a_refused_file_restores_the_rest_and_answers_file_refused",
        &[(FAKE, Scenario::Local("restore-note"))],
    );
    let host = dir.start(0);
    // Three files, the middle one refused: the files on both sides of it are restored.
    let files = [
        ("a.txt", "A draft\n"),
        ("b.txt", "B draft\n"),
        ("c.txt", "C draft\n"),
    ];
    let (matter, _thread, folder) = matter_with_thread(&dir, &host, "m", &files, FAKE);
    let import = matter_events(&host, matter)
        .checkpoint(CheckpointKind::Import)
        .unwrap();
    write::write(&folder.join("a.txt"), b"A edited\n");
    write::write(&folder.join("b.txt"), b"B edited\n");
    write::write(&folder.join("c.txt"), b"C edited\n");
    // The matter may hold its folder by another spelling of the same path (`/private/var`).
    let b = folder.join("b.txt");
    let because = clauseo_matters::RefusedBecause::SecondName;
    // Armed once for each spelling: each arm refuses one write.
    clauseo_matters::test_support::refuse_write(&b, because);
    let canonical = dunce::canonicalize(&b).unwrap();
    if canonical != b {
        clauseo_matters::test_support::refuse_write(canonical, because);
    }
    let read = |name: &str| std::fs::read_to_string(folder.join(name)).unwrap();
    let restore = HostCommand::RestoreVersion {
        thread: None,
        undoes: None,
        matter,
        checkpoint: import,
        scope: RestoreRequest::Matter,
    };

    let error = host.dispatch(restore.clone()).unwrap_err();
    assert_eq!(error.code(), ErrorCode::MatterFileRefused, "{error}");
    assert!(error.retryable());
    assert_eq!(error.files(), [MatterPath::new("b.txt").unwrap()]);
    let refusal = error.refusal().expect("a refusal");
    assert_eq!(
        refusal.because,
        Some(clauseo_protocol::RefusedBecause::SecondName)
    );
    assert_eq!((refusal.refused, refusal.of), (1, 3));
    assert_eq!(error.message(), "matter: 1 of 3 files refused");
    assert_eq!(read("a.txt"), "A draft\n", "the file before it is restored");
    assert_eq!(read("c.txt"), "C draft\n", "the file after it is restored");
    assert_eq!(read("b.txt"), "B edited\n", "the refused file is as it was");
    host.settle();
    let log = matter_events(&host, matter);
    let restored = log
        .events()
        .find_map(|event| match event {
            DomainEvent::VersionRestored(restored) => Some(restored.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no VersionRestored: {}", log.dump()));
    assert_eq!(restored.from, import);
    assert_eq!(
        restored.written,
        [
            MatterPath::new("a.txt").unwrap(),
            MatterPath::new("c.txt").unwrap()
        ]
    );
    assert_eq!(
        log.checkpoint(CheckpointKind::Restore),
        Some(restored.checkpoint)
    );

    assert!(matches!(
        host.ok(restore),
        CommandOutcome::VersionRestored { .. }
    ));
    assert_eq!(read("b.txt"), "B draft\n", "a second restore puts it back");
    host.shutdown();
}
