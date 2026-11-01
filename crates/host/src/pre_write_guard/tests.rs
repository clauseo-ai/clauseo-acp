use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::{MAX_WORDS, run, shell_words};
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

/// What the guard prints for `input`, after checking it returned 0.
fn guard(input: &[u8]) -> String {
    let mut output = Vec::new();
    assert_eq!(run(input, &mut output), 0);
    String::from_utf8(output).unwrap()
}

fn guard_json(hook: &Value) -> String {
    guard(hook.to_string().as_bytes())
}

/// The exact deny line for `name`.
fn deny(name: &str) -> String {
    let reason = format!(
        "{name} is open in Word or another program on this computer. A change made now would be \
         lost when that program saves it. Ask the lawyer to close it, or write to a copy with \
         another name."
    );
    let line = json!({"hookSpecificOutput": {
        "hookEventName": "PreToolUse",
        "permissionDecision": "deny",
        "permissionDecisionReason": reason,
    }});
    format!("{line}\n")
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
    for command in [
        "ls -la",
        "rm Free.docx sub/Note.txt",
        "cat Contract",
        "echo 'Contract.docx is open'",
        "cp *.docx /tmp",
    ] {
        assert_eq!(
            guard_json(&fx.hook("Bash", &json!({"command": command}))),
            "",
            "{command}"
        );
    }
}

/// Only the first [`MAX_WORDS`] words are looked at.
#[test]
fn bash_words_past_the_limit_are_not_checked() {
    let fx = Fixture::new("bash-limit");
    let filler = vec!["x"; MAX_WORDS - 2].join(" ");
    let inside = format!("echo {filler} Contract.docx");
    let past = format!("echo {filler} more Contract.docx");
    assert_eq!(
        guard_json(&fx.hook("Bash", &json!({"command": inside}))),
        deny("Contract.docx")
    );
    assert_eq!(guard_json(&fx.hook("Bash", &json!({"command": past}))), "");
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

/// The deny line for a Codex patch into the matter's `.codex` folder.
fn codex_refused() -> String {
    let reason = "Clauseo doesn't let the agent change the .codex folder in a matter: files there \
                  change what the agent loads and runs.";
    let line = json!({"hookSpecificOutput": {
        "hookEventName": "PreToolUse",
        "permissionDecision": "deny",
        "permissionDecisionReason": reason,
    }});
    format!("{line}\n")
}

fn patch(body: &str) -> String {
    format!("*** Begin Patch\n{body}\n*** End Patch\n")
}

/// clauseo-acp-private#184: each patch `body` writes in the matter's `.codex` folder and is
/// refused under each of Codex's names for `apply_patch`. `{matter}` in a body stands for the
/// matter folder's absolute path.
fn assert_codex_folder_refused(test: &str, bodies: &[&str]) {
    let fx = Fixture::new(test);
    for body in bodies {
        let body = body.replace("{matter}", &fx.path("m"));
        for tool in ["apply_patch", "Edit", "Write"] {
            assert_eq!(
                guard_json(&fx.hook(tool, &json!({"command": patch(&body)}))),
                codex_refused(),
                "{tool} {body}"
            );
        }
    }
}

#[test]
fn a_codex_patch_adding_to_the_codex_folder_is_refused() {
    let inside = Path::new("{matter}").join(".codex").join("config.toml");
    let absolute = format!("*** Add File: {}", inside.display());
    assert_codex_folder_refused(
        "codex-add",
        &[
            "*** Add File: .codex/config.toml\n+approval_policy = \"never\"",
            "*** Add File: .codex",
            "*** Add File: sub/../.codex/config.toml",
            "*** Add File: ./.CODEX/config.toml",
            "*** Add File: .codex. /config.toml",
            &absolute,
        ],
    );
}

#[test]
fn a_codex_patch_updating_the_codex_folder_is_refused() {
    assert_codex_folder_refused(
        "codex-update",
        &["*** Update File: .codex/config.toml\n@@\n-a\n+b"],
    );
}

#[test]
fn a_codex_patch_deleting_from_the_codex_folder_is_refused() {
    assert_codex_folder_refused("codex-delete", &["*** Delete File: .codex/config.toml"]);
}

#[test]
fn a_codex_patch_moving_into_the_codex_folder_is_refused() {
    assert_codex_folder_refused(
        "codex-move-in",
        &["*** Update File: Free.docx\n*** Move to: .codex/hooks.json"],
    );
}

#[test]
fn a_codex_patch_moving_out_of_the_codex_folder_is_refused() {
    assert_codex_folder_refused(
        "codex-move-out",
        &["*** Update File: .codex/config.toml\n*** Move to: notes/config.toml"],
    );
}

/// A patch elsewhere in the matter, and a `.codex` folder outside it, are not refused by the
/// `.codex` rule; nor are Claude's tools and Bash, which keep the held-file rule alone.
#[test]
fn the_codex_folder_rule_covers_only_codex_patches_into_the_matters_folder() {
    let fx = Fixture::new("codex-elsewhere");
    let outside = fx.path("out/.codex/config.toml");
    for body in [
        "*** Add File: notes/codex.md\n+hi".to_owned(),
        "*** Add File: sub/.codex/config.toml".to_owned(),
        "*** Add File: .codexrc".to_owned(),
        "*** Add File: ../.codex/config.toml".to_owned(),
        format!("*** Add File: {outside}"),
    ] {
        assert_eq!(
            guard_json(&fx.hook("apply_patch", &json!({"command": patch(&body)}))),
            "",
            "{body}"
        );
    }
    let inside = fx.path("m/.codex/config.toml");
    assert_eq!(
        guard_json(&fx.hook("Write", &json!({"file_path": inside, "content": "x"}))),
        ""
    );
    assert_eq!(
        guard_json(&fx.hook("Bash", &json!({"command": "touch .codex/config.toml"}))),
        ""
    );
}

#[test]
fn input_it_cannot_read_gets_nothing_and_exit_0() {
    let fx = Fixture::new("malformed");
    let contract = fx.path("m/Contract.docx");
    for input in [
        b"".to_vec(),
        b"not json".to_vec(),
        b"{\"tool_name\": \"Write\"".to_vec(),
        b"[]".to_vec(),
        json!({}).to_string().into_bytes(),
        // No cwd, a relative cwd, a cwd that is not a string.
        json!({"tool_name": "Write", "tool_input": {"file_path": contract}})
            .to_string()
            .into_bytes(),
        json!({"cwd": "m", "tool_name": "Write", "tool_input": {"file_path": contract}})
            .to_string()
            .into_bytes(),
        json!({"cwd": 7, "tool_name": "Write", "tool_input": {"file_path": contract}})
            .to_string()
            .into_bytes(),
        // An unknown tool, and known tools with inputs of the wrong shape.
        fx.hook("Read", &json!({"file_path": contract}))
            .to_string()
            .into_bytes(),
        fx.hook("Write", &json!("Contract.docx"))
            .to_string()
            .into_bytes(),
        fx.hook("Write", &json!({"file_path": 7}))
            .to_string()
            .into_bytes(),
        fx.hook("Bash", &json!({"command": {"rm": contract}}))
            .to_string()
            .into_bytes(),
        vec![0xff, 0xfe, b'{'],
    ] {
        assert_eq!(guard(&input), "", "{}", String::from_utf8_lossy(&input));
    }
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

#[test]
fn shell_words_split_the_simple_shell_way() {
    let split = |command: &str, limit: usize| {
        let mut words = Vec::new();
        shell_words(command, limit, &mut words);
        words
    };
    assert_eq!(
        split(r#"cp  'a b' "c \"d\" \$e \x" f\ g '' h"#, 64),
        ["cp", "a b", r#"c "d" $e \x"#, "f g", "", "h"]
    );
    assert_eq!(split("a 'b c", 64), ["a", "b c"]);
    assert_eq!(split("a b c d", 2), ["a", "b"]);
    assert_eq!(split("trailing\\", 64), ["trailing"]);
    assert_eq!(split("  \t\n ", 64), Vec::<String>::new());
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

/// A Codex patch into the matter's `.codex` folder through a symlink to it is refused.
#[cfg(unix)]
#[test]
fn a_codex_patch_through_a_symlink_to_the_codex_folder_is_refused() {
    let fx = Fixture::new("symlink-codex");
    write::create_dir_all(&fx.matter().join(".codex")).unwrap();
    write::symlink(&fx.matter().join(".codex"), &fx.matter().join("settings")).unwrap();
    assert_eq!(
        guard_json(&fx.hook(
            "apply_patch",
            &json!({"command": patch("*** Add File: settings/config.toml\n+x")})
        )),
        codex_refused()
    );
}
