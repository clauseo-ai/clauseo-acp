//! A finished set-up kept for its rename alone (`install/kept.rs`): a rename refused past its
//! in-place retries fails the try, and the next try (the job's own retry or the lawyer's Retry)
//! re-checks the kept tree and renames it, with no second `npm ci`, no download and no state
//! but `Installing`. A kept tree that fails its check (no marker, a corrupt one, another
//! version's, files missing) is removed and the try runs in full.
#![expect(
    clippy::disallowed_methods,
    reason = "the tests poll for the job's first back-off and spoil the kept tree on disk"
)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use clauseo_agents::InstallState;

use crate::resume::{BACKOFF_FIRST, DEADLINE, rename_fault, start};
use crate::support::{A_PACKAGES, Fixture, agent_a};

/// The rename's in-place waits: doubling to 8 s, then 8 s until 55.75 s.
const RENAME_WAITS: [Duration; 11] = [
    Duration::from_millis(250),
    Duration::from_millis(500),
    Duration::from_secs(1),
    Duration::from_secs(2),
    Duration::from_secs(4),
    Duration::from_secs(8),
    Duration::from_secs(8),
    Duration::from_secs(8),
    Duration::from_secs(8),
    Duration::from_secs(8),
    Duration::from_secs(8),
];
/// The marker a finished set-up's temp tree holds.
const MARKER: &str = ".clauseo-complete";

/// The `.tmp-*` folders under agent A's `install/`, and which of them are marked finished.
fn temp_trees(fx: &Fixture) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(fx.install_root(&agent_a())) else {
        return (Vec::new(), Vec::new());
    };
    let all: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(".tmp-")
        })
        .collect();
    let marked = all
        .iter()
        .filter(|path| path.join(MARKER).is_file())
        .cloned()
        .collect();
    (all, marked)
}

/// Waits until the job's first back-off is armed: its first try has failed.
fn first_try_failed(fx: &Fixture) {
    let start = Instant::now();
    while !fx.timer.install_waits().contains(&BACKOFF_FIRST) {
        assert!(start.elapsed() < DEADLINE, "the first try did not fail");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// From the first `Installing` on, the states are `Installing` and the end alone: never
/// `Downloading` again.
fn no_download_after_installing(seen: &[(clauseo_protocol::AgentId, InstallState)]) {
    let first = seen
        .iter()
        .position(|(_, state)| *state == InstallState::Installing)
        .expect("Installing went out");
    let after = &seen[first..];
    assert!(
        after.iter().all(|(_, state)| matches!(
            state,
            InstallState::Installing | InstallState::Ready { .. } | InstallState::Failed(_)
        )),
        "{seen:?}"
    );
}

/// (a), (d): a rename refused past its 55.75 s of in-place retries fails the try, but the next
/// try renames the kept tree alone: one `npm ci`, one download each, and `Installing` all along.
#[test]
fn a_rename_denied_past_the_in_place_cap_then_allowed_runs_npm_ci_once() {
    let mut fx = Fixture::new("resume-rename-past-cap");
    let (fault, renames) = rename_fault(Some(12), || std::io::Error::from_raw_os_error(5));
    fx.config.install.rename_fault = Some(fault);
    let ci_runs = Arc::new(AtomicUsize::new(0));
    let (agents, events) = start(&fx, &[], &ci_runs);
    agents.install(&agent_a());
    for want in RENAME_WAITS {
        fx.timer.fire_next(want);
    }
    fx.timer.fire_next(BACKOFF_FIRST);
    let (seen, last) = events.until_end(&agent_a());
    assert!(matches!(last, InstallState::Ready { .. }), "{last:?}");
    assert_eq!(
        ci_runs.load(Ordering::SeqCst),
        1,
        "one npm ci across the tries"
    );
    assert_eq!(
        renames.load(Ordering::SeqCst),
        13,
        "twelve refused, then the kept tree's"
    );
    assert_eq!(agents.install_tries(&agent_a()), Some(2));
    no_download_after_installing(&seen);
    for name in A_PACKAGES {
        assert_eq!(fx.requests_for(name).len(), 1, "{name}: one download");
    }
    assert_eq!(
        temp_trees(&fx).0,
        Vec::<PathBuf>::new(),
        "no temp tree left"
    );
}

/// (b): a rename that never works fails as `SetupFailed` after one `npm ci`, keeping the
/// finished tree; the lawyer's Retry, the rename allowed now, renames it alone.
#[test]
fn a_rename_never_allowed_keeps_the_tree_and_a_retry_renames_it_alone() {
    let mut fx = Fixture::new("resume-rename-kept");
    let allowed = Arc::new(AtomicBool::new(false));
    let renames = Arc::new(AtomicUsize::new(0));
    let (open, counted) = (Arc::clone(&allowed), Arc::clone(&renames));
    let fault = move || {
        counted.fetch_add(1, Ordering::SeqCst);
        (!open.load(Ordering::SeqCst)).then(|| std::io::Error::other("the folder is held"))
    };
    fx.config.install.rename_fault = Some(Arc::new(fault));
    let ci_runs = Arc::new(AtomicUsize::new(0));
    let (agents, events) = start(&fx, &[], &ci_runs);
    agents.install(&agent_a());
    fx.timer.fire_next(BACKOFF_FIRST);
    fx.timer.fire_next(Duration::from_secs(8));
    let (seen, last) = events.until_end(&agent_a());
    let InstallState::Failed(failure) = last else {
        panic!("{last:?}");
    };
    assert_eq!(failure.kind.code(), "agent.install_setup_failed");
    assert_eq!(
        ci_runs.load(Ordering::SeqCst),
        1,
        "the later tries only renamed"
    );
    assert_eq!(renames.load(Ordering::SeqCst), 3);
    no_download_after_installing(&seen);
    let (all, marked) = temp_trees(&fx);
    assert_eq!(
        (all.len(), marked.len()),
        (1, 1),
        "the finished tree is kept: {all:?}"
    );

    allowed.store(true, Ordering::SeqCst);
    agents.retry(&agent_a(), false);
    let (seen, last) = events.until_end(&agent_a());
    let InstallState::Ready { .. } = last else {
        panic!("{last:?}");
    };
    assert!(
        !seen
            .iter()
            .any(|(_, state)| matches!(state, InstallState::Downloading { .. })),
        "{seen:?}"
    );
    assert_eq!(ci_runs.load(Ordering::SeqCst), 1, "no second npm ci");
    assert_eq!(
        temp_trees(&fx).0,
        Vec::<PathBuf>::new(),
        "the kept tree was renamed"
    );
    let hash = fx
        .config
        .snapshot
        .install_hash(&agent_a(), fx.config.platform)
        .unwrap();
    let folder = fx.install_root(&agent_a()).join(hash);
    assert!(folder.join("clauseo-receipt.json").is_file());
    assert!(
        !folder.join(MARKER).exists(),
        "the marker is dropped once in place"
    );
    for name in A_PACKAGES {
        assert_eq!(fx.requests_for(name).len(), 1, "{name}: one download");
    }
}

/// (c): the first try's rename fails once, keeping its finished tree; `spoil` then breaks the
/// tree, and the next try finds it unsound, removes it and runs in full: a second `npm ci`.
fn a_spoiled_kept_tree_gets_a_full_try(tag: &str, spoil: fn(&Path)) {
    let mut fx = Fixture::new(tag);
    let (fault, _renames) = rename_fault(Some(1), || std::io::Error::other("the folder is held"));
    fx.config.install.rename_fault = Some(fault);
    let ci_runs = Arc::new(AtomicUsize::new(0));
    let (agents, events) = start(&fx, &[], &ci_runs);
    agents.install(&agent_a());
    first_try_failed(&fx);
    let (_, marked) = temp_trees(&fx);
    assert_eq!(marked.len(), 1, "the first try's tree is kept");
    spoil(&marked[0]);
    fx.timer.fire_next(BACKOFF_FIRST);
    let (_, last) = events.until_end(&agent_a());
    assert!(matches!(last, InstallState::Ready { .. }), "{last:?}");
    assert_eq!(
        ci_runs.load(Ordering::SeqCst),
        2,
        "the spoiled tree was not used"
    );
    assert_eq!(
        temp_trees(&fx).0,
        Vec::<PathBuf>::new(),
        "the spoiled tree is gone"
    );
}

#[test]
fn an_unmarked_kept_tree_gets_a_full_try() {
    a_spoiled_kept_tree_gets_a_full_try("resume-kept-unmarked", |tree| {
        std::fs::remove_file(tree.join(MARKER)).unwrap();
    });
}

#[test]
fn a_corrupt_marker_gets_a_full_try() {
    a_spoiled_kept_tree_gets_a_full_try("resume-kept-corrupt", |tree| {
        std::fs::write(tree.join(MARKER), b"{not json").unwrap();
    });
}

#[test]
fn a_kept_tree_marked_for_another_version_gets_a_full_try() {
    a_spoiled_kept_tree_gets_a_full_try("resume-kept-version", |tree| {
        let text = std::fs::read_to_string(tree.join(MARKER)).unwrap();
        let mut marker: serde_json::Value = serde_json::from_str(&text).unwrap();
        marker["version"] = "0.0.0-another".into();
        std::fs::write(tree.join(MARKER), marker.to_string()).unwrap();
    });
}

#[test]
fn a_half_deleted_kept_tree_gets_a_full_try() {
    a_spoiled_kept_tree_gets_a_full_try("resume-kept-half", |tree| {
        std::fs::remove_dir_all(tree.join("node_modules").join("dep-a")).unwrap();
    });
}
