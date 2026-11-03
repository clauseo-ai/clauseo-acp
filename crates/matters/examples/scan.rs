//! Prints where a folder lives and what a scan before a matter finds in it, for the by-hand
//! check on a network share, a `OneDrive` folder or a `~/Library/CloudStorage` folder:
//! `cargo run -p clauseo-matters --example scan -- <folder>`. Nothing is read or written.

use std::io::Write as _;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use clauseo_matters::{FolderScan, Limits, OsProbe, Retention, Settings, place, scan_folder};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let folder = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .ok_or("usage: scan <folder>")?;
    let settings = Settings {
        // A folder nothing is written to: the scan only checks the folder is not inside it.
        matters_root: std::env::temp_dir().join("clauseo-scan-example-matters"),
        refused_trees: Vec::new(),
        limits: Limits::DEFAULT,
        retention: Retention::KeepAll,
        probe: Arc::new(OsProbe),
    };
    let mut out = std::io::stdout().lock();
    writeln!(out, "place: {:?}", place(&folder))?;
    let mut steps = 0_u64;
    let cancel = AtomicBool::new(false);
    let scan = scan_folder(&folder, &[], &settings, &mut |_| steps += 1, &cancel)?;
    match scan {
        FolderScan::Done(totals) => writeln!(
            out,
            "files: {}, bytes: {}, cloud-only: {}, progress steps: {steps}",
            totals.files, totals.bytes, totals.cloud_only
        )?,
        FolderScan::Cancelled => writeln!(out, "cancelled")?,
    }
    Ok(())
}
