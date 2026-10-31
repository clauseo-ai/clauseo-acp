use std::collections::BTreeSet;

use clauseo_protocol::{EventId, Timestamp, ViewEpoch};

use super::{IdSource, Ids, RandomV7, SequentialIds};

#[test]
fn random_ids_are_v7_and_ordered() {
    let mut ids = Ids::new(Box::new(RandomV7::new()));
    let wall = Timestamp::new(1_760_000_000_123).unwrap();
    let first: EventId = ids.mint(wall).unwrap();
    let second: EventId = ids.mint(wall).unwrap();
    assert!(first.as_uuid() < second.as_uuid());
    let (secs, nanos) = first.as_uuid().get_timestamp().unwrap().to_unix();
    assert_eq!(
        secs * 1000 + u64::from(nanos / 1_000_000),
        1_760_000_000_123
    );
}

#[test]
fn sequential_ids_repeat_per_run_and_differ_across_runs() {
    let wall = Timestamp::new(5_000).unwrap();
    let take = |source: &mut SequentialIds| (0..3).map(|_| source.mint(wall)).collect::<Vec<_>>();
    let a = take(&mut SequentialIds::new());
    assert_eq!(a, take(&mut SequentialIds::new()));
    let b = take(&mut SequentialIds::with_run(1));
    let all: BTreeSet<_> = a.iter().chain(&b).collect();
    assert_eq!(all.len(), 6);
    assert!(a.windows(2).all(|w| w[0] < w[1]));
    for uuid in a.iter().chain(&b) {
        assert_eq!(uuid.get_version_num(), 7);
        ViewEpoch::new(*uuid).unwrap();
    }
    let (secs, nanos) = a[0].get_timestamp().unwrap().to_unix();
    assert_eq!((secs, nanos), (5, 0));
}

#[test]
fn negative_wall_time_still_mints() {
    let mut ids = Ids::new(Box::new(SequentialIds::new()));
    let id: EventId = ids.mint(Timestamp::new(-5).unwrap()).unwrap();
    assert_eq!(id.as_uuid().get_version_num(), 7);
}
