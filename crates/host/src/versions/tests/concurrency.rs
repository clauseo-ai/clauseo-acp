//! Calls that overlap: one job per session on the matter's thread, a call that gives up leaves a
//! job that does nothing, a matter not open is never opened, and one stetdoc run at a time.

use std::collections::VecDeque;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use clauseo_protocol::{CheckpointId, MatterId};
use serde_json::json;

use super::{Fixture, Source, call, ok, read, refused};
use crate::versions::texts::Refusal;
use crate::versions::{
    Logged, MatterRead, Opened, Unreachable, VersionsBackend, VersionsConfig, VersionsSource,
};

/// A matter's thread that runs reads only when the test lets it.
struct Gated {
    inner: Arc<Source>,
    queued: Mutex<VecDeque<(MatterId, MatterRead)>>,
    pushed: Condvar,
}

impl Gated {
    fn new(inner: Arc<Source>) -> Arc<Self> {
        Arc::new(Self {
            inner,
            queued: Mutex::new(VecDeque::new()),
            pushed: Condvar::new(),
        })
    }

    /// Waits until a read is queued, then runs it.
    fn run_next(&self) {
        let mut queued = self.queued.lock().unwrap();
        while queued.is_empty() {
            queued = self.pushed.wait(queued).unwrap();
        }
        let (matter, read) = queued.pop_front().unwrap();
        drop(queued);
        self.inner.on_matter(matter, read).unwrap();
    }

    fn queued(&self) -> usize {
        self.queued.lock().unwrap().len()
    }
}

impl VersionsSource for Gated {
    fn on_matter(&self, matter: MatterId, read: MatterRead) -> Result<(), Unreachable> {
        self.queued.lock().unwrap().push_back((matter, read));
        self.pushed.notify_all();
        Ok(())
    }

    fn logged(&self, matter: MatterId, versions: &[CheckpointId]) -> Logged {
        self.inner.logged(matter, versions)
    }
}

fn gated_backend(fixture: &Fixture, gated: &Arc<Gated>) -> VersionsBackend {
    VersionsBackend::new(VersionsConfig {
        source: Arc::clone(gated) as Arc<dyn VersionsSource>,
        ..fixture.config(None)
    })
}

#[test]
fn a_call_that_gives_up_leaves_a_job_that_does_nothing_and_holds_the_next_call() {
    let fixture = Fixture::new("versions-flight-abandoned");
    let gated = Gated::new(Arc::clone(&fixture.source));
    let backend = gated_backend(&fixture, &gated);
    let flight = &backend.inner.flight;
    flight.set_wait(Duration::from_millis(300));
    let first = call(&backend, "list_versions", json!({}));
    assert_eq!(refused(&first), Refusal::Unavailable.text());
    assert_eq!(gated.queued(), 1);
    flight.set_wait(Duration::from_secs(30));
    std::thread::scope(|scope| {
        let second = scope.spawn(|| call(&backend, "list_versions", json!({})));
        // The second call cannot queue its job while the first's is on the thread.
        assert_eq!(gated.queued(), 1);
        gated.run_next();
        // The abandoned job ran nothing; then the second call's job comes, and runs.
        assert_eq!(flight.ran.load(Ordering::SeqCst), 0);
        gated.run_next();
        let second = second.join().unwrap();
        assert_eq!(ok(&second)["versions"].as_array().unwrap().len(), 1);
    });
    assert_eq!(flight.ran.load(Ordering::SeqCst), 1);
}

#[test]
fn one_job_per_session_is_on_the_matters_thread_at_a_time() {
    let fixture = Fixture::new("versions-flight-one");
    let gated = Gated::new(Arc::clone(&fixture.source));
    let backend = gated_backend(&fixture, &gated);
    std::thread::scope(|scope| {
        let calls: Vec<_> = (0..4)
            .map(|_| scope.spawn(|| call(&backend, "list_versions", json!({ "limit": 1 }))))
            .collect();
        for _ in 0..4 {
            gated.run_next();
            assert!(gated.queued() <= 1, "{}", gated.queued());
        }
        for answer in calls {
            ok(&answer.join().unwrap());
        }
    });
    assert_eq!(backend.inner.flight.ran.load(Ordering::SeqCst), 4);
}

/// A matter's thread that holds no open matter: every read gets none.
struct Closed;

impl VersionsSource for Closed {
    fn on_matter(&self, _matter: MatterId, read: MatterRead) -> Result<(), Unreachable> {
        read.run(None::<Opened<'_>>);
        Ok(())
    }

    fn logged(&self, _matter: MatterId, _versions: &[CheckpointId]) -> Logged {
        Logged::default()
    }
}

#[test]
fn a_matter_not_open_is_refused_until_the_next_turn() {
    let fixture = Fixture::new("versions-flight-closed");
    let backend = VersionsBackend::new(VersionsConfig {
        source: Arc::new(Closed),
        ..fixture.config(None)
    });
    let refusal = call(&backend, "list_versions", json!({}));
    assert_eq!(refused(&refusal), Refusal::NotOpen.text());
    assert!(Refusal::NotOpen.text().contains("next turn"));
    let read_refusal = read(&backend, super::checkpoint(1), "brief.txt");
    assert_eq!(refused(&read_refusal), Refusal::NotOpen.text());
}

/// A stetdoc stand-in that fails when another run of it is going: it marks its run with a
/// folder for a moment, and prints the copy's path as its text.
#[cfg(unix)]
fn lonely_stetdoc(fixture: &Fixture) -> std::path::PathBuf {
    let program = fixture.dir.path().join("lonely-stetdoc");
    let mark = fixture.dir.path().join("running");
    let script = format!(
        "#!/bin/sh\n/bin/mkdir '{mark}' 2>/dev/null || exit 3\n/bin/sleep 0.4\n\
         /bin/rmdir '{mark}'\necho \"text of $3\"\n",
        mark = mark.display()
    );
    crate::write::write_file(&program, script.as_bytes()).unwrap();
    crate::write::set_mode(&program, 0o755).unwrap();
    program
}

#[cfg(unix)]
#[test]
fn one_stetdoc_run_per_session_at_a_time() {
    let mut fixture = Fixture::new("versions-flight-stetdoc");
    fixture.put("one.docx", b"PK one");
    fixture.put("two.docx", b"PK two");
    let v2 = fixture.checkpoint(2);
    let backend = VersionsBackend::new(fixture.config(Some(lonely_stetdoc(&fixture))));
    let answers: Vec<_> = std::thread::scope(|scope| {
        let mut reads = Vec::new();
        for file in ["one.docx", "two.docx"] {
            let backend = backend.clone();
            reads.push(scope.spawn(move || read(&backend, v2, file)));
        }
        reads.into_iter().map(|read| read.join().unwrap()).collect()
    });
    for answer in &answers {
        assert!(
            ok(answer)["text"].as_str().unwrap().starts_with("text of "),
            "{answer}"
        );
    }
}
