//! Scratch folders for the harnesses that need a directory (store_read, matters_read and
//! matters_objects). Shared by those targets through `mod scratch;`; it is not a target itself.

use std::path::{Path, PathBuf};

/// Makes `<temp dir>/clauseo-fuzz-<kind>-<pid>`, empty, and returns it. `std::env::temp_dir`
/// honours TMPDIR. Folders of kind `kind` whose process has exited are removed first (on Linux,
/// where `/proc` shows which pids are alive), so a killed run does not leave its folder behind.
pub fn create(kind: &str) -> PathBuf {
    let prefix = format!("clauseo-fuzz-{kind}-");
    let temp = std::env::temp_dir();
    sweep(&temp, &prefix);
    let dir = temp.join(format!("{prefix}{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch folder in the temp dir");
    dir
}

fn sweep(temp: &Path, prefix: &str) {
    if !cfg!(target_os = "linux") {
        return;
    }
    let Ok(entries) = std::fs::read_dir(temp) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(pid) = name
            .to_str()
            .and_then(|n| n.strip_prefix(prefix))
            .and_then(|p| p.parse::<u32>().ok())
        else {
            continue;
        };
        if pid != std::process::id() && !Path::new(&format!("/proc/{pid}")).exists() {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}
