//! The timing run (ignored; run by hand with `--release`): what skill packs add to a launch
//! through the host (decision record 0040, PR-D). Each run times a prompt to its agent's spawn,
//! which is the launch prep (the folders, the built-in skills, the packs) and the actor's work
//! around it; the agent exits on each prompt, so every prompt launches it again. Cases: no pack,
//! 2 typical packs (3 skills each, about 30 KB), and 16 packs at the caps (4 skills and 253
//! files each, about 7.9 MiB each, 125 MiB). The page cache is warm after the first launch, which
//! is left out.
//!
//! ```text
//! cargo nextest run --release -p clauseo-host --run-ignored only \
//!     -E 'test(timing_launch_with_packs)' --no-capture
//! ```
//!
//! Targets: packs add at most 50 ms (2 typical) and at most 200 ms (the caps).

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use clauseo_process::{LaunchSpec, Launched, Launcher};
use serde_json::json;

use crate::packs::{
    PackHook, exits_on_prompt, hash_of, pack_dir, profile, prompt_until_over, put_pack, skills_on,
};
use crate::profile_rig::{CLAUDE, Wired, agents, settings};
use crate::support::Dir;
use crate::write;

/// Timed launches per case, after one launch that warms the caches.
const RUNS: usize = 9;

/// The clock, for this timing run and the list's timing guard (`skill_packs_cap`) alone.
#[expect(
    clippy::disallowed_methods,
    reason = "the host's timing run reads the clock to time a launch; the core takes time as input"
)]
pub(crate) fn now() -> Instant {
    Instant::now()
}

/// A launcher that notes when each launch reaches it, then launches through [`Wired`].
struct Timed {
    inner: Arc<Wired>,
    at: Mutex<Vec<Instant>>,
}

impl Launcher for Timed {
    fn launch(&self, spec: &LaunchSpec) -> Result<Launched, clauseo_process::Error> {
        self.at.lock().unwrap().push(now());
        self.inner.launch(spec)
    }
}

/// Writes the pack `id`: `skills` skills, each a `SKILL.md` and `refs` text references of
/// `size` bytes. Returns its hash.
fn make_pack(dir: &Dir, id: &str, skills: usize, refs: usize, size: usize) -> String {
    let filler: Vec<u8> = (0..size)
        .map(|n| b"abcdefghij klmnopqrst\n"[n % 22])
        .collect();
    let names: Vec<String> = (0..skills).map(|n| format!("s{n}")).collect();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    put_pack(dir, id, &names, &filler);
    for name in &names {
        let references = pack_dir(dir, id)
            .join("skills")
            .join(name)
            .join("references");
        for n in 1..refs {
            write::write(&references.join(format!("r{n}.md")), &filler);
        }
    }
    hash_of(dir, id)
}

/// The median and the slowest of `runs`.
fn spread(mut runs: Vec<Duration>) -> (Duration, Duration) {
    runs.sort();
    (runs[runs.len() / 2], runs[runs.len() - 1])
}

/// Times [`RUNS`] launches with `count` packs of `skills` skills and `refs` references of
/// `size` bytes each turned on.
fn case(name: &str, count: usize, skills: usize, refs: usize, size: usize) -> Vec<Duration> {
    let dir = Dir::new(&format!("packs_timing_{name}"), &[]);
    let ids: Vec<String> = (0..count).map(|n| format!("kit-{n}")).collect();
    let hashes: Vec<String> = ids
        .iter()
        .map(|id| make_pack(&dir, id, skills, refs, size))
        .collect();
    let enabled: Vec<(&str, &str)> = ids
        .iter()
        .zip(&hashes)
        .map(|(id, hash)| (id.as_str(), hash.as_str()))
        .collect();
    let looping = exits_on_prompt(&dir, "packs-timing");
    let entries = agents(&[(CLAUDE, &looping, json!({}))]);
    settings(&dir, entries, skills_on(&enabled));
    let timed = Arc::new(Timed {
        inner: Wired::new(&dir),
        at: Mutex::new(Vec::new()),
    });
    let hook = PackHook(Arc::new(profile(dir.data())));
    let host = dir.start_launching(0, Arc::<Timed>::clone(&timed), |config| {
        config.with_profile_hook(Box::new(hook))
    });
    let matter = host.create_matter("m", &dir.folder("m", &[]));
    let thread = host.create_thread(matter, CLAUDE);
    let mut runs = Vec::with_capacity(RUNS);
    for run in 0..=RUNS {
        let start = now();
        prompt_until_over(&host, thread);
        let launched = *timed.at.lock().unwrap().last().unwrap();
        if run > 0 {
            runs.push(launched - start);
        }
    }
    host.shutdown();
    let seen = timed.inner.seen();
    let skills_each = seen
        .last()
        .unwrap()
        .packs
        .iter()
        .filter(|file| file.ends_with("SKILL.md"))
        .count();
    assert_eq!(skills_each, count * skills, "every pack loaded");
    runs
}

#[test]
#[ignore = "a timing run, by hand with --release"]
fn timing_launch_with_packs() {
    let none = spread(case("none", 0, 0, 0, 0));
    // 2 packs × 3 skills × (SKILL.md + 2 references of 2.4 KB): about 30 KB.
    let typical = spread(case("typical", 2, 3, 2, 2400));
    // 16 packs × 4 skills × (SKILL.md + 62 references of 33,000 bytes): 253 files and about
    // 7.9 MiB each.
    let caps = spread(case("caps", 16, 4, 62, 33_000));
    for (name, (median, slowest), target) in [
        ("typical", typical, Duration::from_millis(50)),
        ("caps", caps, Duration::from_millis(200)),
    ] {
        println!(
            "{name}: prompt to spawn, median / slowest: {median:?} / {slowest:?}; without packs \
             {:?} / {:?}; added: median {:?}, slowest {:?}; target {target:?}",
            none.0,
            none.1,
            median.saturating_sub(none.0),
            slowest.saturating_sub(none.1),
        );
    }
}
