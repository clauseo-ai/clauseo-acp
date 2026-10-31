//! `checkpoint_per_turn`: one `after_turn` checkpoint with a warm stat cache, on a generated
//! folder of 500 documents (20 KB to 2 MB) of which 3 were rewritten since the last checkpoint.
//! Only the checkpoint is measured; the rewrite is setup. No clock is read: every file's
//! modification time is set to a fixed past time.
//!
//! Beside it, Versions on a large matter (10,000 files of 2 to 20 KB in 100 folders two levels
//! deep) and on a flat one (2,000 files in the folder itself): comparing two checkpoints and a
//! checkpoint with the folder, reading one file of a version, a restore preview, and restores
//! of 100 files in one folder, of 1,000 files across all of them, and of 3 files after 3
//! outside edits. Last, reading one 8 MiB file of a version, text (stored deflated) and
//! `.docx`-like (stored at zlib level 0, so the read is mostly the hash and the copy).
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
        CompareSide, DEFAULT_MARKER, Imported, Limits, Matter, NewMatter, OsProbe, Restore,
        RestoreFiles, Retention, Settings, TurnMeta,
    };
    use clauseo_protocol::{CheckpointId, MatterPath, Timestamp};
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

    /// How many files the large matter holds, in 100 folders `<a>/<b>/`.
    const LARGE: usize = 10_000;
    /// How many files the flat matter holds, all in the folder itself.
    const FLAT: usize = 2_000;

    /// A small document: 2 KB to 20 KB, about 70% `.docx`-like, the rest text.
    fn small_document(rng: &mut Rng) -> (&'static str, Vec<u8>) {
        let len = usize::try_from(2_000 + rng.below(18_000)).unwrap();
        if rng.below(10) < 7 {
            ("docx", rng.contents(b"PK\x03\x04", len))
        } else {
            ("txt", rng.contents(b"", len))
        }
    }

    /// Writes `count` small documents, in 100 folders when `nested`, else in `folder` itself.
    fn generate_small(folder: &Path, rng: &mut Rng, count: usize, nested: bool) -> Vec<String> {
        let mut paths = Vec::with_capacity(count);
        for n in 0..count {
            let (extension, contents) = small_document(rng);
            let dir = if nested {
                format!("{}/{}/", (n / 10) % 10, n % 10)
            } else {
                String::new()
            };
            let rel = format!("{dir}doc-{n:05}.{extension}");
            fs_setup::write(&folder.join(&rel), &contents, at_seconds(BASE_MTIME));
            paths.push(rel);
        }
        paths
    }

    /// Rewrites `which` of `paths` (by index) with new content, at time `second`.
    fn rewrite(folder: &Path, rng: &mut Rng, paths: &[String], which: &[usize], second: u64) {
        for &n in which {
            let (_, contents) = small_document(rng);
            fs_setup::write(
                &folder.join(&paths[n]),
                &contents,
                at_seconds(REWRITE_MTIME + second),
            );
        }
    }

    fn matter_paths(paths: &[String], which: &[usize]) -> Vec<MatterPath> {
        which
            .iter()
            .map(|&n| MatterPath::new(&paths[n]).unwrap())
            .collect()
    }

    /// A turn: `before_turn` (reused) then `after_turn` as checkpoint `n`, after `change`.
    fn turn(matter: &mut Matter, n: u64, change: impl FnOnce()) {
        let meta = |id: u64| TurnMeta {
            checkpoint: checkpoint_id(id),
            thread: "01920000-0000-7000-8000-000000000001".parse().unwrap(),
            turn: "01930000-0000-7000-8000-000000000001".parse().unwrap(),
            at: Timestamp::new(1_700_000_000_000 + 1000 * i64::try_from(id).unwrap()).unwrap(),
        };
        matter.before_turn(&meta(n - 1)).unwrap();
        change();
        matter.after_turn(&meta(n), &BTreeSet::new()).unwrap();
    }

    fn versions(c: &mut Criterion) {
        let base = std::env::temp_dir().join(format!(
            "clauseo-matters-bench-versions-{}",
            std::process::id()
        ));
        fs_setup::remove_all(&base);
        let folder = base.join("matter");
        let mut rng = Rng(0x2545_f491_4f6c_dd1d);
        let paths = generate_small(&folder, &mut rng, LARGE, true);
        let mut matter = import(&base.join("root"), &folder);
        // Checkpoint 3: 3 files in one folder. 5: 1,000 across every folder. 7: all 100 files of
        // folder `0/0`.
        let three = [0, 100, 200];
        let spread: Vec<usize> = (0..LARGE).filter(|n| (n / 100) % 10 == 5).collect();
        let one_folder: Vec<usize> = (0..LARGE)
            .filter(|n| (n / 10) % 10 == 0 && n % 10 == 0)
            .collect();
        assert_eq!(one_folder.len(), 100);
        turn(&mut matter, 3, || {
            rewrite(&folder, &mut rng, &paths, &three, 1);
        });
        turn(&mut matter, 5, || {
            rewrite(&folder, &mut rng, &paths, &spread, 2);
        });
        turn(&mut matter, 7, || {
            rewrite(&folder, &mut rng, &paths, &one_folder, 3);
        });
        let cp = |n: u64| CompareSide::Checkpoint(checkpoint_id(n));

        c.bench_function("compare_large_3", |b| {
            b.iter(|| {
                let changes = matter.compare(cp(1), cp(3)).unwrap();
                assert_eq!(changes.len(), 3);
                changes
            });
        });
        c.bench_function("compare_large_1000", |b| {
            b.iter(|| {
                let changes = matter.compare(cp(3), cp(5)).unwrap();
                assert_eq!(changes.len(), 1000);
                changes
            });
        });
        // One document of a version, two folders down, as the compare screen opens it.
        let deep = MatterPath::new(&paths[5_050]).unwrap();
        c.bench_function("read_file_large", |b| {
            b.iter(|| {
                let mut bytes = Vec::new();
                let len = matter
                    .read_file(checkpoint_id(1), &deep, &mut bytes)
                    .unwrap();
                assert!(len >= 2_000);
                bytes
            });
        });
        // Three edits nothing has recorded yet: compare and preview see them.
        rewrite(&folder, &mut rng, &paths, &[1, 2, 3], 4);
        c.bench_function("compare_folder_large", |b| {
            b.iter(|| {
                let changes = matter.compare(cp(7), CompareSide::Folder).unwrap();
                assert_eq!(changes.len(), 3);
                changes
            });
        });
        c.bench_function("preview_restore_large", |b| {
            b.iter(|| {
                let plan = matter
                    .preview_restore(checkpoint_id(5), RestoreFiles::All)
                    .unwrap();
                assert!(plan.writes.len() >= 100, "{}", plan.writes.len());
                plan
            });
        });

        restores(
            c,
            &mut matter,
            &folder,
            &mut rng,
            &paths,
            &spread,
            &one_folder,
        );
        drop(matter);
        fs_setup::remove_all(&base);
        versions_flat(c, &base, &folder);
    }

    /// The restore benches on the large matter, whose checkpoints 3, 5 and 7 differ in
    /// `spread` (3 and 5) and `one_folder` (5 and 7), and whose folder stands as 7.
    fn restores(
        c: &mut Criterion,
        matter: &mut Matter,
        folder: &Path,
        rng: &mut Rng,
        paths: &[String],
        spread: &[usize],
        one_folder: &[usize],
    ) {
        let chosen_100 = matter_paths(paths, one_folder);
        // Restores alternate between two versions, so each one writes every chosen file.
        let mut next_id: u64 = 1000;
        let mut restore = |matter: &mut Matter, from: u64, files: RestoreFiles<'_>| {
            next_id += 2;
            let request = Restore {
                from: checkpoint_id(from),
                files,
                outside_edits: checkpoint_id(next_id),
                checkpoint: checkpoint_id(next_id + 1),
                at: Timestamp::new(1_800_000_000_000 + i64::try_from(next_id).unwrap()).unwrap(),
            };
            matter.restore(&request).unwrap()
        };
        // The version folder `0/0` stands as now (7 or 5), and the spread files' (5 or 3).
        let mut one_folder_at: u64 = 7;
        let mut spread_at: u64 = 5;
        c.bench_function("restore_files_100", |b| {
            b.iter(|| {
                let from = 12 - one_folder_at;
                one_folder_at = from;
                let done = restore(matter, from, RestoreFiles::Only(&chosen_100));
                assert_eq!(done.written.len(), 100);
                done
            });
        });
        let chosen_1000 = matter_paths(paths, spread);
        c.bench_function("restore_files_1000", |b| {
            b.iter(|| {
                let from = 8 - spread_at;
                spread_at = from;
                let done = restore(matter, from, RestoreFiles::Only(&chosen_1000));
                assert_eq!(done.written.len(), 1000);
                done
            });
        });
        // Three files of folder `0/0`, none of the spread ones.
        let chosen_3 = matter_paths(paths, &[300, 400, 600]);
        let mut second: u64 = 100;
        c.bench_function("restore_after_outside_edits", |b| {
            b.iter_batched(
                || {
                    second += 1;
                    let n = usize::try_from(second).unwrap() % 50 * 7 + 1000;
                    rewrite(folder, rng, paths, &[n, n + 1, n + 2], second);
                    let from = 12 - one_folder_at;
                    one_folder_at = from;
                    from
                },
                |from| {
                    let done = restore(matter, from, RestoreFiles::Only(&chosen_3));
                    assert!(done.outside_edits.is_some());
                    assert_eq!(done.written.len(), 3);
                    done
                },
                BatchSize::PerIteration,
            );
        });
    }

    /// Flat: every file in one tree, so no subtree can be skipped.
    fn versions_flat(c: &mut Criterion, base: &Path, folder: &Path) {
        let cp = |n: u64| CompareSide::Checkpoint(checkpoint_id(n));
        fs_setup::remove_all(base);
        let mut rng = Rng(0x6a09_e667_f3bc_c909);
        let paths = generate_small(folder, &mut rng, FLAT, false);
        let mut matter = import(&base.join("root"), folder);
        turn(&mut matter, 3, || {
            rewrite(folder, &mut rng, &paths, &[0, 700, 1400], 1);
        });
        c.bench_function("compare_flat_3", |b| {
            b.iter(|| {
                let changes = matter.compare(cp(1), cp(3)).unwrap();
                assert_eq!(changes.len(), 3);
                changes
            });
        });
        drop(matter);
        fs_setup::remove_all(base);
    }

    /// Reads one 8 MiB file of checkpoint 1: text, then `.docx`-like.
    fn read_large_file(c: &mut Criterion) {
        const LEN: usize = 8 << 20;
        let base =
            std::env::temp_dir().join(format!("clauseo-matters-bench-read-{}", std::process::id()));
        fs_setup::remove_all(&base);
        let folder = base.join("matter");
        let mut rng = Rng(0xbb67_ae85_84ca_a73b);
        let cases = [("text.txt", &b""[..]), ("brief.docx", &b"PK\x03\x04"[..])];
        for (name, magic) in cases {
            let contents = rng.contents(magic, LEN);
            fs_setup::write(&folder.join(name), &contents, at_seconds(BASE_MTIME));
        }
        let matter = import(&base.join("root"), &folder);
        for (bench, name) in [
            ("read_file_8mib_text", "text.txt"),
            ("read_file_8mib_docx", "brief.docx"),
        ] {
            let path = MatterPath::new(name).unwrap();
            let mut bytes = Vec::with_capacity(LEN);
            c.bench_function(bench, |b| {
                b.iter(|| {
                    bytes.clear();
                    let len = matter
                        .read_file(checkpoint_id(1), &path, &mut bytes)
                        .unwrap();
                    assert_eq!(len, u64::try_from(LEN).unwrap());
                    len
                });
            });
        }
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

    criterion_group! {
        name = versions_benches;
        config = Criterion::default()
            .sample_size(10)
            .measurement_time(Duration::from_secs(5));
        targets = versions, read_large_file
    }
}

#[cfg(test)]
criterion::criterion_main!(bench::benches, bench::versions_benches);

#[cfg(not(test))]
fn main() {}
