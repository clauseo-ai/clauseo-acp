//! The set-aside scan's cost (ignored on CI): `CLAUSEO_WALK_FILES` files (50,000 by default) in
//! two trees, ten files to a folder (5,050 folders) and in 100 folders, each scanned through the
//! tests' volume with the nested list before S15 `PR4a`, with `CLAUDE.md` and `AGENTS.md`, and
//! with `CLAUDE.local.md` and `AGENTS.override.md` too (D-B17). The lists take turns over nine
//! rounds and each keeps its best, so the cache warms them alike. The stand-in runs without its
//! case folding (one `lstat` per probe, as NTFS opens once per probe); on a case-sensitive
//! volume the case variants then go unseen, which a timing doesn't need. Run with
//! `CLAUSEO_WALK_FILES=50000 cargo nextest run -p clauseo-sandbox --run-ignored only
//! -E 'test(scan_timing)' --no-capture`.

use std::path::Path;
use std::time::Duration;

use super::super::scan::{NESTED_NAMES, scan_probing};
use super::posix::Posix;
use super::world;

/// One scan of `root` probing `nested` below it.
#[expect(
    clippy::disallowed_methods,
    reason = "times the scan; the core never reads a clock"
)]
fn timed(root: &Path, nested: &[&str]) -> Duration {
    let started = std::time::Instant::now();
    let scan = scan_probing(&Posix::new(false), root, nested);
    let took = started.elapsed();
    assert_eq!(scan.unscanned, Vec::<std::path::PathBuf>::new());
    took
}

#[test]
#[ignore = "timing row; makes 100,000 files"]
fn scan_timing_by_nested_list() {
    let count: usize = std::env::var("CLAUSEO_WALK_FILES")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(50_000);
    let (w, _root, _data) = world("aside-scan-timing");
    for i in 0..count {
        let tens = i / 10;
        w.dir
            .file(&format!("small/f{}/g{tens}/doc{i}.txt", tens % 50), "x");
        w.dir.file(&format!("wide/f{}/doc{i}.txt", i % 100), "x");
    }
    let seven: Vec<&str> = NESTED_NAMES
        .iter()
        .copied()
        .chain(["CLAUDE.local.md", "AGENTS.override.md"])
        .collect();
    let lists: [(&str, &[&str]); 3] = [
        ("before PR4a (3 names)", &NESTED_NAMES[..3]),
        ("NESTED_NAMES (5)", &NESTED_NAMES),
        ("with CLAUDE.local.md, AGENTS.override.md (7)", &seven),
    ];
    for tree in ["small", "wide"] {
        let root = w.canonical(tree);
        timed(&root, &NESTED_NAMES);
        let mut best = [Duration::MAX; 3];
        for _round in 0..9 {
            for (at, (_, nested)) in lists.iter().enumerate() {
                best[at] = best[at].min(timed(&root, nested));
            }
        }
        for ((label, _), took) in lists.iter().zip(best) {
            println!("files {count}, tree {tree}: {label}: {took:?}");
        }
    }
}
