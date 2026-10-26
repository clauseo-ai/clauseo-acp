//! `checkpoint_per_turn`: one `after_turn` checkpoint with a warm stat cache, on a generated
//! folder of 500 documents (20 KB to 2 MB) of which 3 were rewritten since the last checkpoint.
//! Only the checkpoint is measured; the rewrite is setup. No clock is read: every file's
//! modification time is set to a fixed past time.
//!
//! Everything is under `cfg(test)`, which `cargo bench` sets, so clippy's test allowances
//! (unwrap, expect, indexing) apply.

#[cfg(test)]
mod bench {
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    use std::time::{Duration, SystemTime};

    use clauseo_matters::{
        DEFAULT_MARKER, Imported, Limits, Matter, NewMatter, OsProbe, Retention, Settings, TurnMeta,
    };
    use clauseo_protocol::{CheckpointId, Timestamp};
    use criterion::{BatchSize, Criterion, criterion_group};

    /// How many documents the folder holds.
    const FILES: usize = 500;
    /// How many are rewritten before each measured checkpoint.
    const REWRITTEN: usize = 3;
    /// 2020-01-01T00:00:00Z: every generated file's modification time.
    const BASE_MTIME: u64 = 1_577_836_800;
    /// 2021-01-01T00:00:00Z: the rewritten files' time, plus the iteration in seconds.
    const REWRITE_MTIME: u64 = 1_609_459_200;

    /// The file system writes the bench's setup makes; the crate's own go through its write module.
    #[expect(
        clippy::disallowed_methods,
        reason = "the checkpoint bench generates its folder with std's writes here alone"
    )]
    mod fs_setup {
        use std::fs::{File, FileTimes};
        use std::path::Path;
        use std::time::SystemTime;

        /// Writes `contents` to `path` (parents made) and sets its modification time.
        pub(super) fn write(path: &Path, contents: &[u8], mtime: SystemTime) {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(path, contents).unwrap();
            let file = File::options().write(true).open(path).unwrap();
            file.set_times(FileTimes::new().set_modified(mtime))
                .unwrap();
        }

        /// Removes `path` and everything under it, if it exists.
        pub(super) fn remove_all(path: &Path) {
            if let Err(err) = std::fs::remove_dir_all(path) {
                assert_eq!(err.kind(), std::io::ErrorKind::NotFound, "{err}");
            }
        }
    }

    /// A xorshift64 generator: the same folder on every run.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.0 = x;
            x
        }

        /// A number below `bound`.
        fn below(&mut self, bound: u64) -> u64 {
            self.next() % bound
        }

        /// `len` bytes: random after `magic`, or text-like (compressible) when `magic` is empty.
        fn contents(&mut self, magic: &[u8], len: usize) -> Vec<u8> {
            const WORDS: [&str; 8] = [
                "agreement ",
                "the party ",
                "shall ",
                "indemnify ",
                "notwithstanding ",
                "clause ",
                "hereinafter ",
                "governing law\n",
            ];
            let mut bytes = Vec::with_capacity(len + 16);
            bytes.extend_from_slice(magic);
            while bytes.len() < len {
                let word = self.next();
                if magic.is_empty() {
                    let index = usize::try_from(word % 8).unwrap();
                    bytes.extend_from_slice(WORDS[index].as_bytes());
                } else {
                    bytes.extend_from_slice(&word.to_le_bytes());
                }
            }
            bytes.truncate(len);
            bytes
        }

        /// One document: about 70% `.docx`-like, 15% PDF-like, 15% text; 20 KB to 2 MB, smaller
        /// sizes likelier.
        fn document(&mut self) -> (&'static str, Vec<u8>) {
            let base = 20_000_u64 << self.below(7);
            let len = usize::try_from((base + self.below(base)).min(2_000_000)).unwrap();
            match self.below(100) {
                0..70 => ("docx", self.contents(b"PK\x03\x04", len)),
                70..85 => ("pdf", self.contents(b"%PDF-1.7\n", len)),
                _ => ("txt", self.contents(b"", len)),
            }
        }
    }

    fn at_seconds(seconds: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(seconds)
    }

    /// Checkpoint `n`'s id: the counter in the UUID's low bits.
    fn checkpoint_id(n: u64) -> CheckpointId {
        format!("01900000-0000-7000-8000-{n:012x}").parse().unwrap()
    }

    /// Writes the folder: 500 documents, some in 2 or 3 levels of subfolders.
    fn generate(folder: &Path, rng: &mut Rng) -> Vec<PathBuf> {
        let mut paths = Vec::with_capacity(FILES);
        for n in 0..FILES {
            let (extension, contents) = rng.document();
            let dir = match n % 5 {
                0 => "pleadings/drafts/",
                1 => "correspondence/2019/march/",
                2 => "exhibits/",
                _ => "",
            };
            let path = folder.join(format!("{dir}doc-{n:03}.{extension}"));
            fs_setup::write(&path, &contents, at_seconds(BASE_MTIME));
            paths.push(path);
        }
        paths
    }

    fn import(root: &Path, folder: &Path) -> Matter {
        let settings = Settings {
            matters_root: root.to_owned(),
            limits: Limits::DEFAULT,
            retention: Retention::KeepAll,
            probe: Arc::new(OsProbe),
        };
        let new = NewMatter {
            id: "01910000-0000-7000-8000-000000000001".parse().unwrap(),
            folder: folder.to_owned(),
            created_at: Timestamp::new(1_700_000_000_000).unwrap(),
            marker: DEFAULT_MARKER,
            checkpoint: checkpoint_id(1),
        };
        let cancel = AtomicBool::new(false);
        match Matter::import(settings, new, &[], &mut |_| {}, &cancel).unwrap() {
            Imported::Done { matter, .. } => *matter,
            Imported::Cancelled => unreachable!("no cancel flag was set"),
        }
    }

    fn checkpoint_per_turn(c: &mut Criterion) {
        let base =
            std::env::temp_dir().join(format!("clauseo-matters-bench-{}", std::process::id()));
        fs_setup::remove_all(&base);
        let folder = base.join("matter");
        let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
        let paths = generate(&folder, &mut rng);
        let mut matter = import(&base.join("root"), &folder);
        let thread = "01920000-0000-7000-8000-000000000001".parse().unwrap();
        let turn = "01930000-0000-7000-8000-000000000001".parse().unwrap();
        let no_tools = BTreeSet::new();
        let mut iteration: u64 = 0;

        c.bench_function("checkpoint_per_turn", |b| {
            b.iter_batched(
                || {
                    iteration += 1;
                    for k in 0..REWRITTEN {
                        let n = usize::try_from(iteration).unwrap() * REWRITTEN + k * 167;
                        let (_, contents) = rng.document();
                        let mtime = at_seconds(REWRITE_MTIME + iteration);
                        fs_setup::write(&paths[n % FILES], &contents, mtime);
                    }
                    TurnMeta {
                        checkpoint: checkpoint_id(iteration + 1),
                        thread,
                        turn,
                        at: Timestamp::new(
                            1_700_000_000_000 + 1000 * i64::try_from(iteration).unwrap(),
                        )
                        .unwrap(),
                    }
                },
                |meta| {
                    let checkpoint = matter.after_turn(&meta, &no_tools).unwrap();
                    assert!(checkpoint.changes.len() <= REWRITTEN, "{checkpoint:?}");
                    checkpoint
                },
                BatchSize::PerIteration,
            );
        });
        // The common case beside it: a `before-turn` checkpoint with nothing changed, which only
        // walks the folder against the stat cache and reuses the last checkpoint.
        let unchanged = TurnMeta {
            checkpoint: checkpoint_id(u64::from(u32::MAX)),
            thread,
            turn,
            at: Timestamp::new(1_800_000_000_000).unwrap(),
        };
        c.bench_function("checkpoint_unchanged", |b| {
            b.iter(|| {
                let checkpoint = matter.before_turn(&unchanged).unwrap();
                assert!(checkpoint.changes.is_empty(), "{checkpoint:?}");
                checkpoint
            });
        });
        drop(matter);
        fs_setup::remove_all(&base);
    }

    criterion_group! {
        name = benches;
        config = Criterion::default()
            .sample_size(30)
            .measurement_time(Duration::from_secs(8));
        targets = checkpoint_per_turn
    }
}

#[cfg(test)]
criterion::criterion_main!(bench::benches);

#[cfg(not(test))]
fn main() {}
