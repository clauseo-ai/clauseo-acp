use std::path::PathBuf;

use serde_json::{Value, json};

use std::ffi::OsString;

use super::{DENY, IGNORED, MAX_WORDS, run};
use crate::write;

/// A matter folder `m` holding `Contract.docx`, which Word has open (its owner file
/// `~$ntract.docx` beside it), `Free.docx` and `sub/Note.txt`; and beside it a folder `out`
/// holding `Held.docx`, open too (`~$ld.docx`). Removed on drop.
struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(test: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("clauseo-host-guard-{test}-{}", std::process::id()));
        write::remove_dir_all(&root).unwrap();
        write::create_dir_all(&root.join("m").join("sub")).unwrap();
        write::create_dir_all(&root.join("out")).unwrap();
        for name in [
            "m/Contract.docx",
            "m/~$ntract.docx",
            "m/Free.docx",
            "m/sub/Note.txt",
            "out/Held.docx",
            "out/~$ld.docx",
        ] {
            let path = name
                .split('/')
                .fold(root.clone(), |acc, part| acc.join(part));
            write::write_file(&path, b"x").unwrap();
        }
        Self { root }
    }

    fn matter(&self) -> PathBuf {
        self.root.join("m")
    }

    /// The hook input for `tool` with `input`, with the matter folder as `cwd`.
    fn hook(&self, tool: &str, input: &Value) -> Value {
        json!({
            "session_id": "s-1",
            "transcript_path": "/nowhere/t.jsonl",
            "cwd": self.matter().to_str().unwrap(),
            "permission_mode": "bypassPermissions",
            "hook_event_name": "PreToolUse",
            "tool_name": tool,
            "tool_input": input,
            "tool_use_id": "toolu_1",
        })
    }

    /// The fixture's `out` folder (beside the matter, outside it) as a shell word: forward
    /// slashes, so on Windows it reads `C:/…`, an absolute path the shell keeps as written.
    fn outside(&self) -> String {
        self.path("out").replace('\\', "/")
    }

    fn path(&self, relative: &str) -> String {
        relative
            .split('/')
            .fold(self.root.clone(), |acc, part| acc.join(part))
            .to_str()
            .unwrap()
            .to_owned()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        drop(write::remove_dir_all(&self.root));
    }
}

/// What the guard writes to stderr for `input` with `args`: the reason when it refuses (exit
/// [`DENY`]), an [`IGNORED`] line when it could not read the input and nothing when it lets the
/// tool go (exit 0 both); the test checks the code matches.
fn guard_with(args: &[&str], input: &[u8]) -> String {
    let args: Vec<OsString> = args.iter().map(OsString::from).collect();
    let mut errors = Vec::new();
    let code = run(&args, input, &mut errors);
    let errors = String::from_utf8(errors).unwrap();
    let refused = !errors.is_empty() && !errors.starts_with(IGNORED);
    assert_eq!(code, if refused { DENY } else { 0 }, "{errors}");
    errors
}

/// The exact stderr line for input ignored because `why`.
fn ignored(why: &str) -> String {
    format!("{IGNORED}: {why}. The tool goes on unchecked.\n")
}

/// [`guard_with`] and no arguments, as Codex's hook runs it.
fn guard(input: &[u8]) -> String {
    guard_with(&[], input)
}

fn guard_json(hook: &Value) -> String {
    guard(hook.to_string().as_bytes())
}

/// The exact stderr line refusing a write to `name`.
fn deny(name: &str) -> String {
    format!(
        "{name} is open in Word or another program on this computer. A change made now would be \
         lost when that program saves it. Ask the lawyer to close it, or write to a copy with \
         another name.\n"
    )
}

#[test]
fn claude_file_tools_on_a_held_file_are_refused_with_the_reason() {
    let fx = Fixture::new("claude-held");
    let contract = fx.path("m/Contract.docx");
    for (tool, input) in [
        ("Write", json!({"file_path": contract, "content": "new"})),
        (
            "Edit",
            json!({"file_path": contract, "old_string": "a", "new_string": "b"}),
        ),
        ("MultiEdit", json!({"file_path": contract, "edits": []})),
        (
            "NotebookEdit",
            json!({"notebook_path": contract, "new_source": ""}),
        ),
        (
            "Write",
            json!({"file_path": "Contract.docx", "content": "relative"}),
        ),
    ] {
        assert_eq!(
            guard_json(&fx.hook(tool, &input)),
            deny("Contract.docx"),
            "{tool} {input}"
        );
    }
}

#[test]
fn a_file_with_no_owner_file_is_let_through() {
    let fx = Fixture::new("claude-free");
    for (tool, input) in [
        (
            "Write",
            json!({"file_path": fx.path("m/Free.docx"), "content": "new"}),
        ),
        ("Edit", json!({"file_path": fx.path("m/sub/Note.txt")})),
        (
            "NotebookEdit",
            json!({"notebook_path": fx.path("m/New.ipynb")}),
        ),
    ] {
        assert_eq!(guard_json(&fx.hook(tool, &input)), "", "{tool}");
    }
}

#[test]
fn a_held_file_outside_the_session_folder_is_not_checked() {
    let fx = Fixture::new("outside");
    let held = fx.path("out/Held.docx");
    assert_eq!(
        guard_json(&fx.hook("Write", &json!({"file_path": held}))),
        ""
    );
    assert_eq!(
        guard_json(&fx.hook("Write", &json!({"file_path": "../out/Held.docx"}))),
        ""
    );
    assert_eq!(
        guard_json(&fx.hook("Bash", &json!({"command": format!("rm '{held}'")}))),
        ""
    );
}

#[test]
fn bash_naming_a_held_file_is_refused_quoted_or_not() {
    let fx = Fixture::new("bash-held");
    let contract = fx.path("m/Contract.docx");
    for command in [
        "rm Contract.docx".to_owned(),
        "pandoc -o 'Contract.docx' draft.md".to_owned(),
        "cp draft.docx \"Contract.docx\"".to_owned(),
        "cp draft.docx Con\\tract.docx".to_owned(),
        "echo hi >Contract.docx".to_owned(),
        "tool --out=./Contract.docx".to_owned(),
        format!("sed -i s/a/b/ \"{contract}\""),
    ] {
        assert_eq!(
            guard_json(&fx.hook("Bash", &json!({"command": command}))),
            deny("Contract.docx"),
            "{command}"
        );
    }
    // Codex's argument-list shape.
    assert_eq!(
        guard_json(&fx.hook(
            "Bash",
            &json!({"command": ["bash", "-lc", "rm Contract.docx"]})
        )),
        deny("Contract.docx")
    );
}

#[test]
fn bash_not_naming_a_held_file_is_let_through() {
    let fx = Fixture::new("bash-free");
    let copy_out = format!("cp *.docx {}", fx.outside());
    let commands = [
        "ls -la",
        "rm Free.docx sub/Note.txt",
        "cat Contract",
        "echo 'Contract is open'",
        &copy_out,
    ];
    // `/tmp` is an absolute folder outside the matter only on Unix.
    let tmp = cfg!(unix).then_some("cp *.docx /tmp");
    for command in commands.into_iter().chain(tmp) {
        assert_eq!(
            guard_json(&fx.hook("Bash", &json!({"command": command}))),
            "",
            "{command}"
        );
    }
}

/// Only the first [`MAX_WORDS`] words are looked at as paths; past them, the name backstop
/// still refuses a command naming a held file that is not a plain read.
#[test]
fn bash_words_past_the_limit_are_caught_by_the_name_backstop() {
    let fx = Fixture::new("bash-limit");
    let filler = vec!["x"; MAX_WORDS - 2].join(" ");
    for command in [
        format!("echo {filler} Contract.docx"),
        format!("echo {filler} more Contract.docx"),
    ] {
        assert_eq!(
            guard_json(&fx.hook("Bash", &json!({"command": command}))),
            deny("Contract.docx")
        );
    }
    let read = format!("cat {filler} more Contract.docx");
    assert_eq!(guard_json(&fx.hook("Bash", &json!({"command": read}))), "");
}

#[test]
fn codex_patches_that_change_a_held_file_are_refused_and_adding_a_new_one_is_not() {
    let fx = Fixture::new("codex");
    let contract = fx.path("m/Contract.docx");
    for patch in [
        "*** Begin Patch\n*** Update File: Contract.docx\n@@\n-a\n+b\n*** End Patch\n".to_owned(),
        "*** Begin Patch\n*** Delete File: Contract.docx\n*** End Patch".to_owned(),
        "*** Begin Patch\n*** Update File: Free.docx\n*** Move to: Contract.docx\n*** End Patch"
            .to_owned(),
        format!("*** Begin Patch\n*** Update File: {contract}\n*** End Patch"),
    ] {
        for tool in ["apply_patch", "Edit", "Write"] {
            assert_eq!(
                guard_json(&fx.hook(tool, &json!({"command": patch}))),
                deny("Contract.docx"),
                "{tool} {patch}"
            );
        }
    }
    let add = "*** Begin Patch\n*** Add File: Draft.docx\n+hello\n*** Update File: Free.docx\n\
               *** End Patch";
    assert_eq!(
        guard_json(&fx.hook("apply_patch", &json!({"command": add}))),
        ""
    );
}

/// clauseo-acp-private#162: input it cannot read fails open (exit 0, stdout untouched: `run` has
/// no stdout), with one stderr line naming why and no input content.
#[test]
fn input_it_cannot_read_lets_the_tool_go_with_one_line_saying_why() {
    let fx = Fixture::new("malformed");
    let contract = fx.path("m/Contract.docx");
    let held = fx
        .hook("Write", &json!({"file_path": contract}))
        .to_string();
    let cut = held.get(..held.len() / 2).unwrap().as_bytes().to_vec();
    let bytes = |value: Value| value.to_string().into_bytes();
    let not_json = ignored("stdin was not JSON, or was cut short");
    let no_tool = ignored("it has no tool_name or tool_input");
    let no_folder = ignored("it has no absolute cwd and no --matter was given");
    let no_path = ignored("its tool_input has no file_path, notebook_path or command string");
    let no_command = ignored("its tool_input has no command string or list");
    for (input, line) in [
        (b"".to_vec(), ignored("stdin was empty")),
        (b" \n\t".to_vec(), ignored("stdin was empty")),
        (cut, not_json.clone()),
        (b"not json".to_vec(), not_json.clone()),
        (b"{\"tool_name\": \"Write\"".to_vec(), not_json.clone()),
        (vec![0xff, 0xfe, b'{'], not_json),
        (b"[]".to_vec(), no_folder.clone()),
        (bytes(json!({})), no_folder.clone()),
        // No cwd, a relative cwd, a cwd that is not a string.
        (
            bytes(json!({"tool_name": "Write", "tool_input": {"file_path": contract}})),
            no_folder.clone(),
        ),
        (
            bytes(json!({"cwd": "m", "tool_name": "Write", "tool_input": {"file_path": contract}})),
            no_folder.clone(),
        ),
        (
            bytes(json!({"cwd": 7, "tool_name": "Write", "tool_input": {"file_path": contract}})),
            no_folder,
        ),
        // No tool name or input, then known tools with inputs of the wrong shape.
        (bytes(json!({"cwd": fx.path("m")})), no_tool.clone()),
        (
            bytes(json!({"cwd": fx.path("m"), "tool_name": "Write"})),
            no_tool.clone(),
        ),
        (
            bytes(json!({"cwd": fx.path("m"), "tool_name": 7, "tool_input": {}})),
            no_tool,
        ),
        (
            bytes(fx.hook("Write", &json!("Contract.docx"))),
            no_path.clone(),
        ),
        (bytes(fx.hook("Write", &json!({"file_path": 7}))), no_path),
        (
            bytes(fx.hook("Bash", &json!({"command": {"rm": contract}}))),
            no_command.clone(),
        ),
        (bytes(fx.hook("PowerShell", &json!({}))), no_command),
    ] {
        assert_eq!(guard(&input), line, "{}", String::from_utf8_lossy(&input));
    }
    // A tool the guard does not check is let go quietly.
    assert_eq!(
        guard_json(&fx.hook("Read", &json!({"file_path": contract}))),
        ""
    );
}

/// Stdin that fails to read is ignored the same way.
#[test]
fn stdin_that_fails_to_read_lets_the_tool_go_with_one_line() {
    struct Broken;
    impl std::io::Read for Broken {
        fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("pipe closed"))
        }
    }
    let mut errors = Vec::new();
    assert_eq!(run(&[], Broken, &mut errors), 0);
    assert_eq!(
        String::from_utf8(errors).unwrap(),
        ignored("stdin could not be read")
    );
}

/// Windows: a file another program holds with a share mode that refuses others (Word's way,
/// with no owner file) gets the same reason.
#[cfg(windows)]
#[test]
fn a_file_held_with_a_deny_write_share_mode_is_refused() {
    let fx = Fixture::new("share-mode");
    let free = fx.path("m/Free.docx");
    let handle = write::open_deny_write(std::path::Path::new(&free)).unwrap();
    assert_eq!(
        guard_json(&fx.hook("Write", &json!({"file_path": free}))),
        deny("Free.docx")
    );
    drop(handle);
    assert_eq!(
        guard_json(&fx.hook("Write", &json!({"file_path": free}))),
        ""
    );
}

/// A held file spelled through a symlink to the matter folder is refused, by the permission
/// check's own rule (`held_open_at` resolves both sides), with the matter path it names.
#[cfg(unix)]
#[test]
fn a_held_file_spelled_through_a_symlink_is_refused() {
    let fx = Fixture::new("symlink-held");
    write::symlink(&fx.matter(), &fx.root.join("alias")).unwrap();
    let alias = fx.path("alias/Contract.docx");
    assert_eq!(
        guard_json(&fx.hook("Write", &json!({"file_path": alias}))),
        deny("Contract.docx")
    );
    assert_eq!(
        guard_json(&fx.hook("Bash", &json!({"command": format!("rm {alias}")}))),
        deny("Contract.docx")
    );
}

/// The hook input for `tool` with `input` after the agent ran `cd sub`: Claude Code's `cwd`
/// follows it.
fn hook_in_sub(fx: &Fixture, tool: &str, input: &Value) -> Vec<u8> {
    let mut hook = fx.hook(tool, input);
    hook["cwd"] = json!(fx.path("m/sub"));
    hook.to_string().into_bytes()
}

/// Claude Code's entry passes `--matter`: after a `cd`, a held file of the matter named by an
/// absolute path or from the new folder is still refused. Without `--matter` (an entry from
/// before it), the old rule takes the folder from `cwd` and lets the write through.
#[test]
fn the_matter_argument_keeps_guarding_after_the_agent_changes_folder() {
    let fx = Fixture::new("matter-arg");
    let matter = fx.path("m");
    let args = ["--matter", matter.as_str()];
    let write = hook_in_sub(
        &fx,
        "Write",
        &json!({"file_path": fx.path("m/Contract.docx")}),
    );
    let bash = hook_in_sub(
        &fx,
        "Bash",
        &json!({"command": "echo hi > ../Contract.docx"}),
    );
    let relative = hook_in_sub(&fx, "Edit", &json!({"file_path": "../Contract.docx"}));
    for input in [&write, &bash, &relative] {
        assert_eq!(guard_with(&args, input), deny("Contract.docx"));
        assert_eq!(guard(input), "", "the cwd rule alone misses it");
    }
    // A free file is let through, and a relative --matter is ignored (the cwd rule applies).
    let free = hook_in_sub(&fx, "Write", &json!({"file_path": fx.path("m/Free.docx")}));
    assert_eq!(guard_with(&args, &free), "");
    assert_eq!(guard_with(&["--matter", "m"], &write), "");
}

/// Claude Code's `PowerShell` tool is checked like `Bash`, with backslashes kept.
#[test]
fn powershell_naming_a_held_file_is_refused() {
    let fx = Fixture::new("powershell");
    for command in [
        "Remove-Item Contract.docx".to_owned(),
        "Set-Content -Path 'Contract.docx' -Value hi; Get-Item x".to_owned(),
        "Set-Content -Path:Contract.docx -Value hi".to_owned(),
        "Set-Content -LiteralPath:'Contract.docx' hi".to_owned(),
        format!("Set-Content \"{}\" hi", fx.path("m/Contract.docx")),
    ] {
        assert_eq!(
            guard_json(&fx.hook("PowerShell", &json!({"command": command}))),
            deny("Contract.docx"),
            "{command}"
        );
    }
    assert_eq!(
        guard_json(&fx.hook("PowerShell", &json!({"command": "Get-ChildItem"}))),
        ""
    );
}

/// Shell punctuation stuck to a name does not hide it.
#[test]
fn bash_punctuation_around_a_held_name_is_stripped() {
    let fx = Fixture::new("punctuation");
    for command in [
        "rm Contract.docx; ls",
        "echo hi >>Contract.docx",
        "echo hi >|Contract.docx&",
    ] {
        assert_eq!(
            guard_json(&fx.hook("Bash", &json!({"command": command}))),
            deny("Contract.docx"),
            "{command}"
        );
    }
}

mod allow;
mod backstop;
mod batch;
mod codex;
mod devanagari;
mod fixtures;
mod gaps;
