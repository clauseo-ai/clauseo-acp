//! The protected paths (`protected.rs`): a write of Claude's own settings, instructions, skills
//! or plugins, or of a plugin folder of Clauseo's, is refused, by a file tool and by a shell
//! command in either reading, by path, from a folder change or through the config folder's
//! variable; a read of one goes on, and so does a write beside one.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::guard_with;
use crate::write;

/// The reason line of a refusal for a protected path.
const REFUSED: &str =
    "Clauseo doesn't let the agent change Claude's own settings or Clauseo's plugin files.\n";

/// A matter `m`, Claude's config folder `home` (its `settings.json`, `CLAUDE.md`, `skills` and
/// `plugins`, and `plans`), the built-in plugin and a run's packs plugin under `data`, and a
/// folder `out`. Removed on drop.
struct Layout {
    root: PathBuf,
}

impl Layout {
    fn new(test: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "clauseo-host-guard-protected-{test}-{}",
            std::process::id()
        ));
        write::remove_dir_all(&root).unwrap();
        for folder in ["m", "out", "home/plans", "data/run/r1/work"] {
            write::create_dir_all(&root.join(folder)).unwrap();
        }
        for file in [
            "home/settings.json",
            "home/CLAUDE.md",
            "home/skills/mine/SKILL.md",
            "home/plugins/known.json",
            "data/profile/claude-plugin/skills/legal-drafting/SKILL.md",
            "data/run/r1/skill-packs/skills/nda/SKILL.md",
            "m/a.md",
        ] {
            let path = Self::under(&root, file);
            write::create_dir_all(path.parent().unwrap()).unwrap();
            write::write_file(&path, b"x").unwrap();
        }
        Self { root }
    }

    fn under(root: &Path, relative: &str) -> PathBuf {
        relative
            .split('/')
            .fold(root.to_path_buf(), |at, part| at.join(part))
    }

    /// `relative` under the layout, as the OS writes it.
    fn path(&self, relative: &str) -> String {
        Self::under(&self.root, relative)
            .to_str()
            .unwrap()
            .to_owned()
    }

    /// `relative` under the layout with `/` between parts, as a Bash command writes it.
    fn slashed(&self, relative: &str) -> String {
        self.path(relative).replace('\\', "/")
    }

    /// The protected paths, as the launch names them (`agent::loads::guard_protected`).
    fn protected(&self) -> Vec<PathBuf> {
        crate::agent::loads::guard_protected(
            Path::new(&self.path("home")),
            Path::new(&self.path("data")),
        )
    }

    /// Claude Code's arguments for the matter: `--matter`, then the config folder, each
    /// protected path, and the run's packs plugin, which only a file tool may not change.
    fn args(&self) -> Vec<String> {
        let mut args = vec![
            "--matter".to_owned(),
            self.path("m"),
            "--claude-home".to_owned(),
            self.path("home"),
        ];
        for path in self.protected() {
            args.push("--protected".to_owned());
            args.push(path.to_str().unwrap().to_owned());
        }
        args.push("--protected-edit".to_owned());
        args.push(self.path("data/run/r1/skill-packs"));
        args
    }

    /// What the guard says for `tool` with `input`, in `acceptEdits`, from the matter, with
    /// `args`.
    fn guard(&self, args: &[String], tool: &str, input: &Value) -> String {
        let hook = json!({
            "session_id": "s-1",
            "cwd": self.path("m"),
            "permission_mode": "acceptEdits",
            "hook_event_name": "PreToolUse",
            "tool_name": tool,
            "tool_input": input,
        });
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        guard_with(&args, hook.to_string().as_bytes())
    }

    /// [`Self::guard`] of a shell command `command` with Claude Code's arguments.
    fn shell(&self, tool: &str, command: &str) -> String {
        self.guard(&self.args(), tool, &json!({"command": command}))
    }
}

impl Drop for Layout {
    fn drop(&mut self) {
        drop(write::remove_dir_all(&self.root));
    }
}

/// The places a file tool may not write: each kind of protected path, existing or not, by
/// absolute path, relative to the matter, or through a `..`.
const PROTECTED_FILES: [&str; 15] = [
    "home/settings.json",
    "home/settings.local.json",
    "home/CLAUDE.md",
    "home/.claude.json",
    "home/skills/new/SKILL.md",
    "home/commands/x.md",
    "home/agents/x.md",
    "home/rules/x.md",
    "home/workflows/x.js",
    "home/agent-memory/x/MEMORY.md",
    "home/hooks/x.json",
    "home/plugins/p/hooks/hooks.json",
    "data/profile/claude-plugin/skills/planted/SKILL.md",
    "data/profile/claude-plugin/.claude-plugin/plugin.json",
    "data/run/r1/skill-packs/skills/nda/SKILL.md",
];

#[test]
fn file_tools_writing_a_protected_path_are_refused() {
    let fx = Layout::new("files");
    let args = fx.args();
    for relative in PROTECTED_FILES {
        let path = fx.path(relative);
        for (tool, input) in [
            ("Write", json!({"file_path": path, "content": "x"})),
            (
                "Edit",
                json!({"file_path": path, "old_string": "x", "new_string": "y"}),
            ),
            ("MultiEdit", json!({"file_path": path, "edits": []})),
            ("NotebookEdit", json!({"notebook_path": path})),
        ] {
            assert_eq!(fx.guard(&args, tool, &input), REFUSED, "{tool} {relative}");
        }
    }
    for path in [
        "../home/settings.json",
        "../home/plans/../skills/x/SKILL.md",
    ] {
        let input = json!({"file_path": path, "content": "x"});
        assert_eq!(fx.guard(&args, "Write", &input), REFUSED, "{path}");
    }
}

/// A write beside a protected path, or elsewhere in the config folder or the data folder, is
/// not this rule's.
#[test]
fn a_write_beside_a_protected_path_goes_on() {
    let fx = Layout::new("beside");
    let args = fx.args();
    for relative in [
        "home/settings.json.bak",
        "home/settings",
        "home/skills2/x.md",
        "home/plans/plan.md",
        "data/profile/other.txt",
        "data/run/r1/work/draft.md",
        "m/a.md",
    ] {
        let input = json!({"file_path": fx.path(relative), "content": "x"});
        assert_eq!(fx.guard(&args, "Write", &input), "", "{relative}");
    }
}

/// Bash commands that write a protected path, by path, after a folder change, or through the
/// config folder's variable in each of its forms, are refused.
#[test]
fn bash_writing_a_protected_path_is_refused() {
    let fx = Layout::new("bash");
    let home = fx.slashed("home");
    let plugin = fx.slashed("data/profile/claude-plugin");
    for command in [
        format!("echo x > {home}/settings.json"),
        format!("echo x >> '{home}/settings.local.json'"),
        format!("cp a.md \"{home}/skills/x/SKILL.md\""),
        format!("mkdir -p {home}/plugins/p && cp a.md {home}/plugins/p/x.json"),
        format!("cd {home} && echo x > settings.local.json"),
        format!("cd {home}/skills; touch new.md"),
        format!("pushd {plugin} && rm -rf skills"),
        "echo x > \"$CLAUDE_CONFIG_DIR/settings.json\"".to_owned(),
        "cp a.md ${CLAUDE_CONFIG_DIR}/skills/x/SKILL.md".to_owned(),
        "echo x > %CLAUDE_CONFIG_DIR%/CLAUDE.md".to_owned(),
        "cd \"$CLAUDE_CONFIG_DIR\" && sed -i s/a/b/ CLAUDE.md".to_owned(),
        format!("bash -c 'echo x > {home}/settings.json'"),
    ] {
        assert_eq!(fx.shell("Bash", &command), REFUSED, "{command}");
    }
}

/// The same under PowerShell's reading: `Set-Content`, `Out-File`, `Copy-Item` and the like by
/// path, after `Set-Location` or `cd`, and through `$env:CLAUDE_CONFIG_DIR` in either form.
#[test]
fn powershell_writing_a_protected_path_is_refused() {
    let fx = Layout::new("powershell");
    let home = fx.path("home");
    let plugin = fx.path("data/profile/claude-plugin");
    for command in [
        format!(r"Set-Content -Path {home}\settings.json -Value x"),
        format!(r"Out-File -FilePath '{home}\CLAUDE.md' -InputObject x"),
        format!(r"Copy-Item a.md {home}\skills\x\SKILL.md"),
        format!(r"New-Item -ItemType File -Path {plugin}\skills\x\SKILL.md"),
        format!(r"Set-Location {home}; Set-Content settings.local.json x"),
        r"Set-Content -Path $env:CLAUDE_CONFIG_DIR\settings.json -Value x".to_owned(),
        r"Copy-Item a.md ${env:CLAUDE_CONFIG_DIR}\plugins\p.json".to_owned(),
        r"cd $env:CLAUDE_CONFIG_DIR; Remove-Item CLAUDE.md".to_owned(),
        r"'x' > $env:CLAUDE_CONFIG_DIR\settings.json".to_owned(),
    ] {
        assert_eq!(fx.shell("PowerShell", &command), REFUSED, "{command}");
    }
}

/// A command that only reads a protected path goes on, in both readings, by path and through
/// the variable; and so does a copy of one out to a file elsewhere.
#[test]
fn a_read_of_a_protected_path_goes_on() {
    let fx = Layout::new("reads");
    let home = fx.slashed("home");
    let out = fx.slashed("out");
    for command in [
        format!("cat {home}/settings.json"),
        format!("ls {home}/skills"),
        format!("head -5 {home}/CLAUDE.md | wc -l"),
        format!("cp {home}/settings.json {out}/copy.json"),
        "cat \"$CLAUDE_CONFIG_DIR/settings.json\"".to_owned(),
    ] {
        assert_eq!(fx.shell("Bash", &command), "", "{command}");
    }
    let home = fx.path("home");
    for command in [
        format!(r"Get-Content {home}\settings.json"),
        format!(r"Test-Path {home}\skills"),
        r"Get-Content $env:CLAUDE_CONFIG_DIR\CLAUDE.md".to_owned(),
    ] {
        assert_eq!(fx.shell("PowerShell", &command), "", "{command}");
    }
}

/// With no protected path in the arguments (Codex's hook, or an entry without them), the rule
/// refuses nothing.
#[test]
fn without_protected_paths_nothing_is_refused_for_them() {
    let fx = Layout::new("none");
    let args = vec!["--matter".to_owned(), fx.path("m")];
    let input = json!({"file_path": fx.path("home/settings.json"), "content": "x"});
    assert_eq!(fx.guard(&args, "Write", &input), "");
    let home = fx.slashed("home");
    let command = json!({"command": format!("echo x > {home}/settings.json")});
    assert_eq!(fx.guard(&args, "Bash", &command), "");
}

/// The arguments of Claude Code's real entry (`agent::loads::guard_entry`) carry the protected
/// paths and the config folder: a write to one is refused through them.
#[test]
fn the_real_entry_arguments_refuse_a_protected_write() {
    let fx = Layout::new("entry");
    let protected = fx.protected();
    let (matter, home) = (PathBuf::from(fx.path("m")), PathBuf::from(fx.path("home")));
    let entry = crate::agent::loads::guard_entry(crate::agent::loads::GuardHook {
        program: Path::new(&crate::testing::abs("/opt/clauseo/clauseo-desktop")),
        matter: &matter,
        ask_all: false,
        protected: &protected,
        protected_edit: &[],
        home: Some(&home),
    })
    .unwrap();
    let args: Vec<String> = entry["hooks"][0]["args"]
        .as_array()
        .unwrap()
        .iter()
        .skip(1)
        .map(|arg| arg.as_str().unwrap().to_owned())
        .collect();
    let input = json!({"file_path": fx.path("home/settings.json"), "content": "x"});
    assert_eq!(fx.guard(&args, "Write", &input), REFUSED);
    let command = json!({"command": "echo x > $CLAUDE_CONFIG_DIR/CLAUDE.md"});
    assert_eq!(fx.guard(&args, "Bash", &command), REFUSED);
}

/// A write named past the words read one by one (after 64 other words) is refused by the whole
/// text's tail, in both readings; so is one through an NTFS stream suffix. A read past them
/// goes on.
#[test]
fn a_protected_write_past_the_word_limit_or_through_a_stream_is_refused() {
    let fx = Layout::new("long");
    let filler = "x ".repeat(crate::pre_write_guard::MAX_WORDS + 10);
    let home = fx.slashed("home");
    let bash = [
        format!("echo {filler}; cp a.md {home}/settings.json"),
        format!("echo {filler}; echo x > '{home}/skills/x/SKILL.md'"),
    ];
    for command in &bash {
        assert_eq!(fx.shell("Bash", command), REFUSED, "{command}");
    }
    let home = fx.path("home");
    for command in [
        format!(r"Write-Output {filler}; Copy-Item a.md {home}\settings.json"),
        format!(r"Set-Content -Path {home}\settings.json::$DATA -Value x"),
        format!(r"Set-Content -Path {home}\CLAUDE.md:x -Value x"),
    ] {
        assert_eq!(fx.shell("PowerShell", &command), REFUSED, "{command}");
    }
    let home = fx.slashed("home");
    let read = format!("echo {filler}; cat {home}/settings.json");
    assert_eq!(fx.shell("Bash", &read), "", "{read}");
}

/// A pack's script in the run's packs plugin runs, and a command may read the plugin, in both
/// readings: only a file tool may not change it (`--protected-edit`).
#[test]
fn a_pack_script_runs_and_its_plugin_is_closed_to_file_tools_alone() {
    let fx = Layout::new("packs");
    let script = fx.slashed("data/run/r1/skill-packs/skills/nda/scripts/fill.py");
    for command in [
        format!("python3 {script} m/a.md"),
        format!("grep -rn x {}", fx.slashed("data/run/r1/skill-packs")),
    ] {
        assert_eq!(fx.shell("Bash", &command), "", "{command}");
    }
    let script = fx.path("data/run/r1/skill-packs/skills/nda/scripts/fill.py");
    let command = format!("python {script} m\\a.md");
    assert_eq!(fx.shell("PowerShell", &command), "", "{command}");
    let input = json!({"file_path": script, "content": "x"});
    assert_eq!(fx.guard(&fx.args(), "Write", &input), REFUSED);
}

/// A protected value the guard cannot read (not absolute) is left out with a line saying so;
/// the guard still decides on the rest.
#[test]
fn a_protected_value_left_out_is_said() {
    let fx = Layout::new("skipped");
    let mut args = fx.args();
    args.extend([
        "--protected".to_owned(),
        "relative/settings.json".to_owned(),
    ]);
    let skipped = format!("{}\n", crate::pre_write_guard::SKIPPED);
    let input = json!({"file_path": fx.path("m/a.md"), "content": "x"});
    assert_eq!(fx.guard(&args, "Write", &input), skipped);
    let input = json!({"file_path": fx.path("home/settings.json"), "content": "x"});
    assert_eq!(
        fx.guard(&args, "Write", &input),
        format!("{skipped}{REFUSED}")
    );
}
