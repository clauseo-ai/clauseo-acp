//! `connector_signin_host`: the host's share of a sign-in against the fake connector, the figure
//! behind the `connector_signin_host_p95` budget row. Each iteration builds a fresh
//! `Connectors` (so discovery runs), begins the sign-in, drives the authorize step with a plain
//! GET at the fake (the browser's part), and finishes: the callback, the token exchange, the
//! keychain write (a `MemoryStore` here, so the real keychain's time is not in it) and the state
//! file. Wall clock: it reports and never fails a pull request.

#![expect(
    clippy::unwrap_used,
    reason = "bench setup and timed calls: a failure ends the bench run with its error"
)]

use std::sync::Arc;
use std::thread;
use std::time::Duration;

use clauseo_connectors::fake::{FakeConnector, FakeOptions, ScratchDir};
use clauseo_connectors::{Connectors, MemoryStore, SecretStore};
use criterion::{Criterion, criterion_group, criterion_main};

fn signin(c: &mut Criterion) {
    let fake = FakeConnector::start(FakeOptions::default()).unwrap();
    let scratch = ScratchDir::new("bench-signin").unwrap();
    let store: Arc<dyn SecretStore> = Arc::new(MemoryStore::new());
    let mut group = c.benchmark_group("connectors");
    group.sample_size(30);
    group.measurement_time(Duration::from_secs(5));
    group.bench_function("connector_signin_host", |b| {
        b.iter(|| {
            let config = fake.config(scratch.state_file());
            let connectors = Connectors::new(config, Arc::clone(&store)).unwrap();
            let handle = connectors.get("case-law").unwrap();
            let pending = handle.begin_sign_in(None).unwrap();
            let url = pending.authorize_url().clone();
            let finisher = thread::spawn(move || pending.finish());
            let page = fake.drive_authorize(&url).unwrap();
            assert_eq!(page.status, 200);
            finisher.join().unwrap().unwrap();
        });
    });
    group.finish();
}

criterion_group!(benches, signin);
criterion_main!(benches);
