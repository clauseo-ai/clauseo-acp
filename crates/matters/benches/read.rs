//! `read_file_8mib_text` and `read_file_8mib_docx`: reading one 8 MiB file of a version, text
//! (stored deflated) and `.docx`-like (stored at zlib level 0, so the read is mostly the hash
//! and the copy). The folder is generated with fixed contents and times, as in `checkpoint.rs`.
//!
//! Everything is under `cfg(test)`, which `cargo bench` sets, so clippy's test allowances
//! (unwrap, expect, indexing) apply.

#[cfg(test)]
#[path = "common/mod.rs"]
mod common;

#[cfg(test)]
mod bench {
    use std::time::Duration;

    use clauseo_protocol::MatterPath;
    use criterion::{Criterion, criterion_group};

    use crate::common::{BASE_MTIME, Rng, at_seconds, checkpoint_id, fs_setup, import};

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
            .sample_size(10)
            .measurement_time(Duration::from_secs(5));
        targets = read_large_file
    }
}

#[cfg(test)]
criterion::criterion_main!(bench::benches);

#[cfg(not(test))]
fn main() {}
