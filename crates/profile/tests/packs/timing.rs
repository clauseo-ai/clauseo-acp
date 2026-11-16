//! The timing run (ignored; run by hand with `--release`, see decision record 0040): what a
//! launch adds to read, check, hash and render its packs (`load`) and write its own plugin
//! from memory (`write_packs`).
//!
//! ```text
//! cargo test --release -p clauseo-profile --test packs -- --ignored --nocapture timing
//! ```
//!
//! Targets (0040), load and write together: 2 typical packs (3 skills each, about 30 KB) in
//! 50 ms or less; 16 packs at the caps (8 MiB and 256 files each, 128 MiB) in 200 ms or less.
//! The page cache is warm, as for every launch after the first.

use std::time::Duration;

use clauseo_profile::packs::{self, Loaded, PackEntry};

use super::{Scratch, entry, manifest_json, skill_md, support};

/// How many timed runs each case takes; the report gives the median and the slowest.
const RUNS: usize = 15;

/// Writes `count` packs of `skills` skills each, each skill with `refs` reference files of
/// `ref_size` bytes, and returns their entries.
fn make_packs(
    scratch: &Scratch,
    count: usize,
    skills: usize,
    refs: usize,
    ref_size: usize,
) -> Vec<PackEntry> {
    let filler: Vec<u8> = (0..ref_size)
        .map(|n| b"abcdefghij klmnopqrst\n"[n % 22])
        .collect();
    (0..count)
        .map(|n| {
            let id = format!("kit-{n}");
            let names: Vec<String> = (0..skills).map(|s| format!("s{s}")).collect();
            let names: Vec<&str> = names.iter().map(String::as_str).collect();
            scratch.put(
                &id,
                "clauseo-pack.json",
                manifest_json(&id, &names, true).as_bytes(),
            );
            for name in &names {
                scratch.put(
                    &id,
                    &format!("skills/{name}/SKILL.md"),
                    skill_md(name, "Does it.").as_bytes(),
                );
                for r in 0..refs {
                    scratch.put(&id, &format!("skills/{name}/references/r{r}.md"), &filler);
                }
            }
            entry(&scratch.inspect(&id).unwrap())
        })
        .collect()
}

/// The median and the slowest of `runs`.
fn spread(mut runs: Vec<Duration>) -> (Duration, Duration) {
    runs.sort();
    (runs[runs.len() / 2], runs[runs.len() - 1])
}

/// Times [`RUNS`] launches of `entries`: `load` (read, check, hash, render in memory), then
/// `write_packs` into a fresh plugin folder, each timed apart and together.
fn time_case(scratch: &Scratch, name: &str, entries: &[PackEntry], target: Duration) {
    let mut loads = Vec::with_capacity(RUNS);
    let mut writes = Vec::with_capacity(RUNS);
    let mut totals = Vec::with_capacity(RUNS);
    let run_root = scratch.join("run");
    for run in 0..RUNS {
        let (loaded, load): (Loaded, Duration) =
            support::timed(|| packs::load(&scratch.packs(), entries, false));
        assert!(loaded.skipped.is_empty(), "{:?}", loaded.skipped);
        assert_eq!(loaded.packs.len(), entries.len());
        let plugin = run_root.join(format!("{name}-{run}")).join("skill-packs");
        let (written, write) = support::timed(|| packs::write_packs(&plugin, &loaded.packs));
        written.unwrap();
        support::remove_dir_all(&run_root).unwrap();
        loads.push(load);
        writes.push(write);
        totals.push(load + write);
    }
    let bytes: u64 = std::fs::read_dir(scratch.packs())
        .unwrap()
        .map(|pack| dir_size(&pack.unwrap().path()))
        .sum();
    let (load, write, total) = (spread(loads), spread(writes), spread(totals));
    println!(
        "{name}: {} packs, {} KiB; median / slowest: load {:?} / {:?}; write {:?} / {:?}; launch {:?} / {:?}; target {target:?}",
        entries.len(),
        bytes / 1024,
        load.0,
        load.1,
        write.0,
        write.1,
        total.0,
        total.1,
    );
}

/// The bytes of every file under `dir`.
fn dir_size(dir: &std::path::Path) -> u64 {
    std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            let meta = std::fs::symlink_metadata(entry.path()).unwrap();
            if meta.is_dir() {
                dir_size(&entry.path())
            } else {
                meta.len()
            }
        })
        .sum()
}

#[test]
#[ignore = "a timing run, by hand with --release"]
fn timing_load_and_write() {
    let scratch = Scratch::new("timing-typical");
    // 2 packs × 3 skills × (SKILL.md + 2 references of 2.4 KB): about 30 KB.
    let entries = make_packs(&scratch, 2, 3, 2, 2400);
    time_case(&scratch, "typical", &entries, Duration::from_millis(50));
    drop(scratch);
    let scratch = Scratch::new("timing-caps");
    // 16 packs × 4 skills × (SKILL.md + 62 references of 33,000 bytes): 253 files and about
    // 7.9 MiB each, all text, so the shell-injection scan reads every byte.
    let entries = make_packs(&scratch, 16, 4, 62, 33_000);
    time_case(&scratch, "caps", &entries, Duration::from_millis(200));
}
