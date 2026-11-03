//! `checkpoint_cost`: what a checkpoint costs on a folder of papers, through the calls the host
//! makes: `Matter::import` for a matter's first checkpoint, `before_turn` and `after_turn` for
//! each turn, and with `--undo` `undo` and `restore`. On Windows, Defender scans every new file,
//! so the loose-object count is the number to watch.
//!
//! ```text
//! cargo run --release -p clauseo-matters --example checkpoint_cost -- <folder> \
//!     [--papers N] [--seed S] [--undo]
//! ```
//!
//! `<folder>` must not exist or must be empty. The history goes in `<folder>-history`, which
//! must not exist; delete both folders afterwards. Defaults: 200 papers, seed 1.
//!
//! The fixture, for N papers: 3/5 `.docx` of 40 to 400 KB; 3/10 PDFs, most of 100 to 200 KB and
//! N/40 scans of 1 to 2.5 MB; the rest `.txt`, `.rtf` and `.xlsx` of 10 to 200 KB. Papers sit in
//! five folders. Bytes come from a xorshift generator with the seed, after each format's magic.
//! The mix totals about 45 MB for 200 papers: the PDF sizes in the brief alone (60 files of up to
//! 3 MB) would make 120 MB, so most PDFs are kept small and a few are scans.
//!
//! The session: 20 turns. Each turn rewrites 1 to 3 papers: the second one of a turn gets 4 KB
//! appended, the others get 16 bytes changed in the middle. Every fourth turn also adds a 200 KB
//! exhibit. Each turn is `before_turn`, the edits, then `after_turn` with no tool paths; both
//! calls are timed.
//!
//! Sizes are apparent file sizes; on Windows, size on disk rounds each file up to the cluster.

use std::collections::BTreeSet;
use std::error::Error;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use clauseo_matters::{
    Checkpoint, CheckpointOutcome, DEFAULT_MARKER, Imported, Limits, Matter, NewMatter, OsProbe,
    Restore, RestoreFiles, Retention, Settings, TurnMeta, UndoRequest,
};
use clauseo_protocol::{
    CheckpointId, FileChoice, FileDecision, MatterId, ThreadId, Timestamp, TurnId,
};

/// The turns of the session.
const TURNS: usize = 20;
const TURN_EPOCH_MS: i64 = 1_700_000_000_000;

type Failure = Box<dyn Error>;

/// The command line.
struct Args {
    folder: PathBuf,
    papers: usize,
    seed: u64,
    undo: bool,
}

/// The fixture's writes: the one place the example writes files. The crate writes its own through
/// its write module, which this example cannot reach.
#[expect(
    clippy::disallowed_methods,
    reason = "the example writes its fixture and its edits with std, as the benches do"
)]
mod fixture;
mod store;

use fixture::{Rng, add_exhibit, change_middle, contents, plan, write_fixture};
use store::measure;

/// Checkpoint ids: the counter in the UUID's low bits, as the crate's tests make them.
fn checkpoint_id(n: u64) -> Result<CheckpointId, Failure> {
    Ok(format!("01900000-0000-7000-8000-{n:012x}").parse()?)
}

/// Hands out checkpoint ids once each, after the root's (1).
struct Ids(u64);

impl Ids {
    fn take(&mut self) -> Result<CheckpointId, Failure> {
        self.0 += 1;
        checkpoint_id(self.0)
    }
}

/// The matter's id, fixed so the history folder is the same on every run.
fn matter_id() -> Result<MatterId, Failure> {
    Ok("01910000-0000-7000-8000-000000000001".parse()?)
}

fn thread_id() -> Result<ThreadId, Failure> {
    Ok("01920000-0000-7000-8000-000000000001".parse()?)
}

fn turn_id() -> Result<TurnId, Failure> {
    Ok("01930000-0000-7000-8000-000000000001".parse()?)
}

/// The example's clock: it times the calls, which the core never reads.
#[expect(
    clippy::disallowed_methods,
    reason = "this example times the calls; the core takes time as input"
)]
fn now() -> Instant {
    Instant::now()
}

/// Runs `run`, returning its result and how long it took.
fn timed<T, E>(run: impl FnOnce() -> Result<T, E>) -> Result<(T, Duration), E> {
    let start = now();
    let value = run()?;
    Ok((value, start.elapsed()))
}

/// One turn's measurements.
struct Row {
    turn: usize,
    edits: usize,
    before: Duration,
    after: Duration,
    changes: usize,
    reused: bool,
}

/// Runs the 20-turn session on `matter`, printing each row, and returns the rows and the last
/// turn's checkpoints: its `before-turn` id and its `after-turn` id.
fn session(
    matter: &mut Matter,
    folder: &Path,
    rels: &[String],
    rng: &mut Rng,
    ids: &mut Ids,
    out: &mut impl Write,
) -> Result<(Vec<Row>, CheckpointId, CheckpointId), Failure> {
    let (thread, turn) = (thread_id()?, turn_id()?);
    let mut rows = Vec::with_capacity(TURNS);
    let mut last = (checkpoint_id(1)?, checkpoint_id(1)?);
    writeln!(out, "turn  edits  before ms   after ms  changes  reused")?;
    for n in 1..=TURNS {
        let at = Timestamp::new(TURN_EPOCH_MS + 1_000 * i64::try_from(n)?)?;
        let before_meta = TurnMeta {
            checkpoint: ids.take()?,
            thread,
            turn,
            at,
        };
        let (before, before_time) = timed(|| matter.before_turn(&before_meta))?;

        let mut picks = BTreeSet::new();
        let wanted = 1 + rng.below(3);
        while picks.len() < wanted {
            picks.insert(rng.below(rels.len()));
        }
        let mut edits = 0;
        for (position, index) in picks.iter().enumerate() {
            let Some(rel) = rels.get(*index) else {
                continue;
            };
            let path = folder.join(rel);
            if position == 1 {
                fixture::append(&path, &contents(rng, b"", 4_096, false))?;
            } else {
                change_middle(&path)?;
            }
            edits += 1;
        }
        if n % 4 == 0 {
            add_exhibit(folder, n, rng)?;
            edits += 1;
        }

        let after_meta = TurnMeta {
            checkpoint: ids.take()?,
            thread,
            turn,
            at,
        };
        let (after, after_time): (Checkpoint, Duration) =
            timed(|| matter.after_turn(&after_meta, &BTreeSet::new()))?;
        let row = Row {
            turn: n,
            edits,
            before: before_time,
            after: after_time,
            changes: after.changes.len(),
            reused: matches!(after.outcome, CheckpointOutcome::Reused(_)),
        };
        writeln!(
            out,
            "{:>4}  {:>5}  {:>9}  {:>9}  {:>7}  {}",
            row.turn,
            row.edits,
            ms(row.before),
            ms(row.after),
            row.changes,
            if row.reused { "yes" } else { "no" },
        )?;
        rows.push(row);
        last = (before.outcome.id(), after.outcome.id());
    }
    Ok((rows, last.0, last.1))
}

/// A duration in milliseconds, one decimal.
fn ms(duration: Duration) -> String {
    format!("{:.1}", duration.as_secs_f64() * 1000.0)
}

/// Min, median and max of `values` (empty gives zeros).
fn spread(mut values: Vec<Duration>) -> (Duration, Duration, Duration) {
    values.sort();
    let (Some(&min), Some(&max)) = (values.first(), values.last()) else {
        return (Duration::ZERO, Duration::ZERO, Duration::ZERO);
    };
    let middle = values.len() / 2;
    let median = match (values.get(middle.wrapping_sub(1)), values.get(middle)) {
        (Some(&low), Some(&high)) if values.len().is_multiple_of(2) => (low + high) / 2,
        (_, Some(&high)) => high,
        _ => Duration::ZERO,
    };
    (min, median, max)
}

/// Bytes as megabytes, one decimal (decimal megabytes).
fn mb(bytes: u64) -> String {
    format!("{}.{} MB", bytes / 1_000_000, (bytes / 100_000) % 10)
}

fn args() -> Result<Args, Failure> {
    let mut folder = None;
    let mut papers = 200;
    let mut seed = 1;
    let mut undo = false;
    let mut rest = std::env::args().skip(1);
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--papers" => papers = rest.next().ok_or("--papers needs a number")?.parse()?,
            "--seed" => seed = rest.next().ok_or("--seed needs a number")?.parse()?,
            "--undo" => undo = true,
            _ if folder.is_none() && !arg.starts_with("--") => folder = Some(PathBuf::from(arg)),
            _ => return Err(format!("unknown argument {arg}; see the example's header").into()),
        }
    }
    let folder =
        folder.ok_or("usage: checkpoint_cost <folder> [--papers N] [--seed S] [--undo]")?;
    if papers < 10 {
        return Err("--papers needs at least 10".into());
    }
    Ok(Args {
        folder: std::path::absolute(folder)?,
        papers,
        seed,
        undo,
    })
}

/// Refuses a folder that is not empty and a history folder that exists; makes the folder.
fn prepare(folder: &Path, history: &Path) -> Result<(), Failure> {
    if folder.exists() && fs::read_dir(folder)?.next().is_some() {
        return Err(format!("refusing: {} is not empty", folder.display()).into());
    }
    if history.exists() {
        return Err(format!("refusing: {} exists; delete it first", history.display()).into());
    }
    fixture::folder(folder)?;
    Ok(())
}

fn settings(history: &Path) -> Settings {
    Settings {
        matters_root: history.to_owned(),
        refused_trees: Vec::new(),
        limits: Limits::DEFAULT,
        retention: Retention::KeepAll,
        probe: Arc::new(OsProbe),
    }
}

fn main() -> Result<(), Failure> {
    let args = args()?;
    let mut out = io::stdout();
    let history = {
        let mut name = args.folder.as_os_str().to_owned();
        name.push("-history");
        PathBuf::from(name)
    };
    prepare(&args.folder, &history)?;

    let mut rng = Rng::new(args.seed.max(1));
    let planned = plan(args.papers, &mut rng);
    let total: u64 = planned.iter().map(|paper| paper.len as u64).sum();
    let profile = if cfg!(debug_assertions) {
        "DEBUG build (use --release)"
    } else {
        "release"
    };
    writeln!(
        out,
        "checkpoint_cost: {} papers, {} total, seed {}, {profile}, {}/{}",
        args.papers,
        mb(total),
        args.seed,
        std::env::consts::OS,
        std::env::consts::ARCH,
    )?;
    writeln!(out, "folder:  {}", args.folder.display())?;
    writeln!(out, "history: {}", history.display())?;
    let rels = write_fixture(&args.folder, &planned, &mut rng)?;

    let settings = settings(&history);
    let new = NewMatter {
        id: matter_id()?,
        folder: args.folder.clone(),
        created_at: Timestamp::new(TURN_EPOCH_MS)?,
        marker: DEFAULT_MARKER,
        checkpoint: checkpoint_id(1)?,
    };
    let cancel = AtomicBool::new(false);
    let (imported, first) =
        timed(|| Matter::import(settings, new, &[], &mut |_progress| {}, &cancel))?;
    let Imported::Done { mut matter, .. } = imported else {
        return Err("the import was cancelled".into());
    };
    let store = measure(&history.join(matter_id()?.to_string()).join("history.git"))?;
    writeln!(out)?;
    writeln!(
        out,
        "first checkpoint (import, root commit): {} ms",
        ms(first)
    )?;
    writeln!(
        out,
        "  loose objects after it: {} files, {} ; objects/ {} ; history.git {}",
        store.loose,
        mb(store.loose_bytes),
        mb(store.objects_bytes),
        mb(store.git_dir_bytes),
    )?;
    writeln!(out)?;

    let mut ids = Ids(1);
    let (rows, before_last, after_last) = session(
        &mut matter,
        &args.folder,
        &rels,
        &mut rng,
        &mut ids,
        &mut out,
    )?;
    let (min_before, median_before, max_before) = spread(rows.iter().map(|r| r.before).collect());
    let (min_after, median_after, max_after) = spread(rows.iter().map(|r| r.after).collect());
    writeln!(out)?;
    writeln!(out, "per-turn ms      min     median        max")?;
    writeln!(
        out,
        "before_turn  {:>8}  {:>9}  {:>9}",
        ms(min_before),
        ms(median_before),
        ms(max_before)
    )?;
    writeln!(
        out,
        "after_turn   {:>8}  {:>9}  {:>9}",
        ms(min_after),
        ms(median_after),
        ms(max_after)
    )?;
    let store = measure(&history.join(matter_id()?.to_string()).join("history.git"))?;
    writeln!(out)?;
    writeln!(
        out,
        "loose objects at the end: {} files, {} ; objects/ {} ; history.git {}",
        store.loose,
        mb(store.loose_bytes),
        mb(store.objects_bytes),
        mb(store.git_dir_bytes),
    )?;

    if args.undo {
        undo_and_restore(&mut matter, &mut ids, before_last, after_last, &mut out)?;
    }
    Ok(())
}

/// Times an undo of the last turn and a restore of the first checkpoint.
fn undo_and_restore(
    matter: &mut Matter,
    ids: &mut Ids,
    before: CheckpointId,
    after: CheckpointId,
    out: &mut impl Write,
) -> Result<(), Failure> {
    let (thread, turn) = (thread_id()?, turn_id()?);
    let at = Timestamp::new(1_800_000_000_000)?;
    let decisions: Vec<FileDecision> = matter
        .changes_between(before, after)?
        .iter()
        .map(|change| FileDecision {
            path: change.path().clone(),
            choice: FileChoice::Undo,
        })
        .collect();
    let request = UndoRequest {
        before,
        after,
        decisions: &decisions,
        thread: Some(thread),
        turn: Some(turn),
        outside_edits: ids.take()?,
        checkpoint: ids.take()?,
        at,
    };
    let (undone, undo_time) = timed(|| matter.undo(&request))?;

    let restore = Restore {
        from: checkpoint_id(1)?,
        files: RestoreFiles::All,
        outside_edits: ids.take()?,
        checkpoint: ids.take()?,
        at,
    };
    let (restored, restore_time) = timed(|| matter.restore(&restore))?;
    writeln!(out)?;
    writeln!(
        out,
        "undo of turn {TURNS}: {} ms, {} files put back",
        ms(undo_time),
        undone.restored.len()
    )?;
    writeln!(
        out,
        "restore of the first checkpoint: {} ms, {} files written",
        ms(restore_time),
        restored.written.len()
    )?;
    Ok(())
}
