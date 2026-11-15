use super::RunSweep;
use crate::agent::codex_memories::{MARKER, codex_home};
use crate::write;

/// The start-up sweep clears what earlier builds left of Codex's memories, before any launch.
#[test]
fn the_start_up_sweep_clears_codex_memories() {
    let root = std::env::temp_dir().join(format!("clauseo-sweep-memories-{}", std::process::id()));
    write::remove_dir_all(&root).unwrap();
    let data = root.join("data");
    let home = codex_home(&data);
    write::create_dir_all(&home.join("memories")).unwrap();
    write::write_file(&home.join("memories_1.sqlite"), b"rows").unwrap();
    write::write_file(&home.join("auth.json"), b"kept").unwrap();
    let sweep = RunSweep::default();
    sweep.once(&data);
    sweep.join_delete();
    assert!(!home.join("memories_1.sqlite").exists());
    assert!(!home.join("memories").exists());
    assert!(home.join("auth.json").exists());
    assert!(home.parent().unwrap().join(MARKER).exists());
    write::remove_dir_all(&root).unwrap();
}
