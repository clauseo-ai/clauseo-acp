//! `papers_*`: listing a matter's papers for its file list, at the protocol's cap
//! (`MAX_PAPERS`, 2,000), on generated folders of 1,000 and 10,000 small documents in 100
//! folders two levels deep (as the Versions benches in `checkpoint.rs` lay them out), and of
//! 10,000 in the folder itself. The import is setup; only the listing is measured.
//!
//! Everything is under `cfg(test)`, which `cargo bench` sets, so clippy's test allowances
//! (unwrap, expect, indexing) apply.

#[cfg(test)]
#[path = "common/mod.rs"]
mod common;

#[cfg(test)]
mod bench {
    use std::path::Path;
    use std::time::Duration;

    use clauseo_protocol::MAX_PAPERS;
    use criterion::{Criterion, criterion_group};

    use crate::common::{BASE_MTIME, Rng, at_seconds, fs_setup, import};

    /// Writes `count` documents of 2 KB to 20 KB, in 100 folders `<a>/<b>/` when `nested`.
    fn generate(folder: &Path, count: usize, nested: bool) {
        let mut rng = Rng(0x2545_f491_4f6c_dd1d);
        for n in 0..count {
            let len = usize::try_from(2_000 + rng.next() % 18_000).unwrap();
            let contents = rng.contents(b"PK\x03\x04", len);
            let dir = if nested {
                format!("{}/{}/", (n / 10) % 10, n % 10)
            } else {
                String::new()
            };
            let path = folder.join(format!("{dir}doc-{n:05}.docx"));
            fs_setup::write(&path, &contents, at_seconds(BASE_MTIME));
        }
    }

    fn one(c: &mut Criterion, name: &str, count: usize, nested: bool) {
        let base = std::env::temp_dir().join(format!(
            "clauseo-matters-bench-papers-{name}-{}",
            std::process::id()
        ));
        fs_setup::remove_all(&base);
        let folder = base.join("matter");
        generate(&folder, count, nested);
        let matter = import(&base.join("root"), &folder);
        let limit = usize::try_from(MAX_PAPERS).unwrap();
        let expected = count.min(limit);
        c.bench_function(&format!("papers_{name}"), |b| {
            b.iter(|| {
                let papers = matter.papers(limit).unwrap();
                assert_eq!(papers.papers.len(), expected);
                papers
            });
        });
        drop(matter);
        fs_setup::remove_all(&base);
    }

    fn papers(c: &mut Criterion) {
        one(c, "1000", 1_000, true);
        one(c, "10000", 10_000, true);
        one(c, "10000_flat", 10_000, false);
    }

    criterion_group! {
        name = benches;
        config = Criterion::default()
            .sample_size(30)
            .measurement_time(Duration::from_secs(5));
        targets = papers
    }
}

#[cfg(test)]
criterion::criterion_main!(bench::benches);

#[cfg(not(test))]
fn main() {}
