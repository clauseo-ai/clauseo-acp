//! The file-tool fence through the host (`file_fence`): a Claude launch with the guard writes
//! the fence file in its run folder and passes it to the guard's hook, which now runs before
//! the file tools that read and search too.

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::{Value, json};

use crate::profile_rig::{CLAUDE, Wired, agent_lines, agents, settings, template, turn};
use crate::support::Dir;

/// The program the hook entry names; nothing runs it.
// unix-paths-ok: the hook entry carries it as text; nothing opens it
const PROGRAM: &str = "/opt/clauseo/clauseo-desktop";

#[test]
fn a_claude_launch_writes_the_fence_and_hooks_the_file_tools_with_it() {
    let dir = Dir::new("file_fence_hook", &[]);
    let entry = json!({
        "matcher": "Read|Edit|Write|MultiEdit|NotebookEdit|Grep|Glob|Bash|PowerShell",
        "hooks": [{
            "type": "command",
            "command": PROGRAM,
            "args": ["--pre-write-guard", "--matter", "{{any}}", "--fence", "{{any}}", "{{any}}"],
            "timeout": 10,
        }],
    });
    let params = json!({"_meta": {"claudeCode": {"options": {"settings": {
        "hooks": {"PreToolUse": [entry]},
    }}}}});
    let claude = template(
        &dir,
        "fence",
        &agent_lines("fence", "session/new", &params, &[]),
    );
    settings(&dir, agents(&[(CLAUDE, &claude, json!({}))]), json!({}));
    let launcher = Wired::new(&dir);
    let host = dir.start_launching(0, Arc::<Wired>::clone(&launcher), |config| {
        config.with_pre_write_guard(PathBuf::from(PROGRAM))
    });
    let matter = host.create_matter("m", &dir.folder("m", &[]));
    turn(&host, host.create_thread(matter, CLAUDE));
    let runs = std::fs::read_dir(dir.data_dir().join("run")).unwrap();
    let fences: Vec<Vec<u8>> = runs
        .filter_map(Result::ok)
        .filter_map(|run| std::fs::read(run.path().join("file-fence.json")).ok())
        .collect();
    host.shutdown();
    let [fence] = fences.as_slice() else {
        panic!("one fence file: {} found", fences.len());
    };
    let fence: Value = serde_json::from_slice(fence).unwrap();
    let data = &fence["roots"][0];
    assert_eq!(data["kind"], json!("data"), "{fence}");
    let real = dunce::canonicalize(dir.data_dir()).unwrap();
    assert_eq!(
        data["spellings"][0],
        json!(real.to_str().unwrap()),
        "{fence}"
    );
    let read = data["read"].as_array().unwrap();
    for pattern in ["run/*/file-fence.json", "state.sqlite*", "matters/**"] {
        assert!(read.contains(&json!(pattern)), "{pattern}: {fence}");
    }
}
