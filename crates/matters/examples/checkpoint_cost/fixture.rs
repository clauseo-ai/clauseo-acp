//! The fixture: the papers' plan and bytes, and the edits the session makes. They are written
//! with std, as the benches write theirs; the crate's write module is the crate's own.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::Path;

use super::Failure;

/// Words for text-like papers: compressible, as prose is.
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
/// The folders the papers sit in, the root first.
const DIRS: [&str; 5] = [
    "",
    "Pleadings/",
    "Correspondence/2019/",
    "Exhibits/A/",
    "Exhibits/B/",
];
const DOCX_MAGIC: &[u8] = b"PK\x03\x04";
const PDF_MAGIC: &[u8] = b"%PDF-1.7\n";
/// One paper to write: its folder-relative path, its size and its contents' kind.
pub(super) struct Planned {
    pub(super) rel: String,
    pub(super) len: usize,
    magic: &'static [u8],
    text: bool,
}

/// A xorshift64 generator: the same fixture for the same seed.
pub(super) struct Rng(u64);

impl Rng {
    /// The generator for `seed`.
    pub(super) fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    /// A number below `bound`, which is at least 1.
    pub(super) fn below(&mut self, bound: usize) -> usize {
        let bound = u64::try_from(bound.max(1)).unwrap_or(1);
        usize::try_from(self.next() % bound).unwrap_or(0)
    }

    /// A length from `low` to `high`, both included.
    pub(super) fn between(&mut self, low: usize, high: usize) -> usize {
        low + self.below(high - low + 1)
    }
}

/// Creates `path` with `contents`, its parents too.
pub(super) fn create(path: &Path, contents: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, contents)
}

/// Creates `path` and its parents.
pub(super) fn folder(path: &Path) -> io::Result<()> {
    fs::create_dir_all(path)
}

/// Appends `contents` to `path`.
pub(super) fn append(path: &Path, contents: &[u8]) -> io::Result<()> {
    OpenOptions::new()
        .append(true)
        .open(path)?
        .write_all(contents)
}

/// `len` bytes after `magic`: random words, or text-like ones when `text`.
pub(super) fn contents(rng: &mut Rng, magic: &[u8], len: usize, text: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(len + 16);
    out.extend_from_slice(magic);
    while out.len() < len {
        let word = rng.next();
        if text {
            let index = usize::try_from(word % 8).unwrap_or(0);
            out.extend_from_slice(WORDS.get(index).copied().unwrap_or("").as_bytes());
        } else {
            out.extend_from_slice(&word.to_le_bytes());
        }
    }
    out.truncate(len);
    out
}

/// The papers of a fixture of `count`: a 3/5 docx, 3/10 PDF and the rest other, in `DIRS`.
pub(super) fn plan(count: usize, rng: &mut Rng) -> Vec<Planned> {
    let docx = count * 3 / 5;
    let pdf = count * 3 / 10;
    let scans = count / 40;
    (0..count)
        .map(|n| {
            let dir = DIRS.get(n % DIRS.len()).copied().unwrap_or("");
            let (extension, len, magic, text) = if n < docx {
                ("docx", rng.between(40_000, 400_000), DOCX_MAGIC, false)
            } else if n < docx + pdf {
                let len = if n - docx < scans {
                    rng.between(1_000_000, 2_500_000)
                } else {
                    rng.between(100_000, 200_000)
                };
                ("pdf", len, PDF_MAGIC, false)
            } else {
                match n % 3 {
                    0 => ("txt", rng.between(10_000, 200_000), b"".as_slice(), true),
                    1 => ("rtf", rng.between(10_000, 200_000), b"".as_slice(), true),
                    _ => ("xlsx", rng.between(10_000, 200_000), DOCX_MAGIC, false),
                }
            };
            Planned {
                rel: format!("{dir}paper-{n:03}.{extension}"),
                len,
                magic,
                text,
            }
        })
        .collect()
}

/// Writes the fixture under `folder`, returning the folder-relative paths in order.
pub(super) fn write_fixture(
    folder: &Path,
    planned: &[Planned],
    rng: &mut Rng,
) -> Result<Vec<String>, Failure> {
    let mut rels = Vec::with_capacity(planned.len());
    for paper in planned {
        let bytes = contents(rng, paper.magic, paper.len, paper.text);
        create(&folder.join(&paper.rel), &bytes)?;
        rels.push(paper.rel.clone());
    }
    Ok(rels)
}

/// Changes 16 bytes in the middle of `path`.
pub(super) fn change_middle(path: &Path) -> Result<(), Failure> {
    let mut bytes = fs::read(path)?;
    let middle = bytes.len() / 2;
    for byte in bytes.iter_mut().skip(middle).take(16) {
        *byte = byte.wrapping_add(1);
    }
    create(path, &bytes)?;
    Ok(())
}

/// Adds a 200 KB exhibit under `Exhibits/A/` for `turn`, returning its folder-relative path.
pub(super) fn add_exhibit(folder: &Path, turn: usize, rng: &mut Rng) -> Result<String, Failure> {
    let rel = format!("Exhibits/A/exhibit-turn-{turn:02}.pdf");
    let bytes = contents(rng, PDF_MAGIC, 200_000, false);
    create(&folder.join(&rel), &bytes)?;
    Ok(rel)
}
