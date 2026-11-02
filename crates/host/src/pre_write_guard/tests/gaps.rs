//! clauseo-acp-private#162's live check: a `cd` inside a shell command, and a `..` through a
//! folder that does not exist, hid a held file from the guard.

use serde_json::json;

use super::{Fixture, deny, guard_json, guard_with};
use crate::write;

/// Refused, with the matter as `cwd` and as Claude Code's `--matter` too.
fn assert_refused(fx: &Fixture, tool: &str, input: &serde_json::Value) {
    let hook = fx.hook(tool, input);
    assert_eq!(guard_json(&hook), deny("Contract.docx"), "{tool} {input}");
    let matter = fx.path("m");
    assert_eq!(
        guard_with(&["--matter", &matter], hook.to_string().as_bytes()),
        deny("Contract.docx"),
        "--matter {tool} {input}"
    );
}

fn bash(command: &str) -> serde_json::Value {
    json!({"command": command})
}

/// Gap 1: a held file named from the folder an earlier `cd` moved to is refused, whether that
/// folder exists or not yet.
#[test]
fn a_held_file_named_after_a_cd_in_the_command_is_refused() {
    let fx = Fixture::new("gap-cd");
    let commands = [
        "mkdir -p Drafts && cd Drafts && echo hi > ../Contract.docx",
        "cd Drafts && echo hi > ../Contract.docx",
        "cd Drafts && cd .. && echo hi > Contract.docx",
        "cd Drafts; cd ../sub || true\necho hi > ../Contract.docx",
        "cd \"$(pwd)/Drafts\" && echo hi > ../Contract.docx",
        "pushd Drafts/deeper && echo hi > ../../Contract.docx",
    ];
    for present in [false, true] {
        if present {
            write::create_dir_all(&fx.matter().join("Drafts").join("deeper")).unwrap();
        }
        for command in commands {
            assert_refused(&fx, "Bash", &bash(command));
        }
        // Codex's argument-list shape.
        assert_refused(
            &fx,
            "Bash",
            &json!({"command": ["bash", "-lc", "cd Drafts && echo hi > ../Contract.docx"]}),
        );
        assert_refused(
            &fx,
            "PowerShell",
            &bash(r"Set-Location Drafts; Set-Content ..\Contract.docx hi"),
        );
        assert_refused(
            &fx,
            "PowerShell",
            &bash(r"cd Drafts\deeper; Set-Content -Path:..\..\Contract.docx hi"),
        );
    }
}

/// A `cd` out of the matter takes the words after it out too: a held file there is not checked.
#[test]
fn a_held_file_outside_the_matter_after_a_cd_is_let_through() {
    let fx = Fixture::new("gap-cd-out");
    let out = fx.path("out");
    for command in [
        format!("cd {out} && echo hi > {out}/Held.docx"),
        format!("cd {out} && echo hi > Held.docx"),
        "cd ../out && echo hi > Held.docx".to_owned(),
        "cd sub && echo hi > Note.txt".to_owned(),
    ] {
        assert_eq!(
            guard_json(&fx.hook("Bash", &bash(&command))),
            "",
            "{command}"
        );
    }
}

/// Gap 2: a `..` after a folder that does not exist is resolved by its text, for every tool.
#[test]
fn a_held_file_named_through_a_missing_folder_and_dotdot_is_refused() {
    let fx = Fixture::new("gap-dotdot");
    let absolute = format!("{}/Drafts/../Contract.docx", fx.path("m"));
    for path in [
        "Drafts/../Contract.docx",
        "./Drafts/more/../../Contract.docx",
        "sub/Drafts/../../Contract.docx",
        absolute.as_str(),
    ] {
        assert_refused(&fx, "Write", &json!({"file_path": path, "content": "x"}));
        assert_refused(
            &fx,
            "Edit",
            &json!({"file_path": path, "old_string": "a", "new_string": "b"}),
        );
        assert_refused(&fx, "Bash", &bash(&format!("echo hi > {path}")));
        assert_refused(&fx, "Bash", &bash(&format!("cp x.docx '{path}'")));
        assert_refused(
            &fx,
            "apply_patch",
            &json!({"command": format!("*** Begin Patch\n*** Update File: {path}\n*** End Patch")}),
        );
    }
    assert_refused(
        &fx,
        "PowerShell",
        &bash(r"Set-Content Drafts\..\Contract.docx hi"),
    );
    // A free file, or a held one outside the matter, is still let through.
    for path in ["Drafts/../Free.docx", "Drafts/../../out/Held.docx"] {
        assert_eq!(
            guard_json(&fx.hook("Write", &json!({"file_path": path}))),
            "",
            "{path}"
        );
        assert_eq!(
            guard_json(&fx.hook("Bash", &bash(&format!("echo hi > {path}")))),
            "",
            "{path}"
        );
    }
}

/// A `cd` through a symlink in the matter to a folder outside it: a held file there is outside
/// the matter, so it is let through, while one in the matter is still refused.
#[cfg(unix)]
#[test]
fn a_cd_through_a_symlink_out_of_the_matter_is_followed() {
    let fx = Fixture::new("gap-cd-symlink");
    write::symlink(&fx.root.join("out"), &fx.matter().join("link")).unwrap();
    assert_eq!(
        guard_json(&fx.hook("Bash", &bash("cd link && echo hi > Held.docx"))),
        ""
    );
    assert_refused(
        &fx,
        "Bash",
        &bash("cd link && echo hi > Contract.docx; cd .. && echo hi > Contract.docx"),
    );
}

/// Review round 1: each command reaches `Contract.docx` from a folder the guard must not lose,
/// past a `cd` that never runs, a subshell, a comment, a heredoc, a prefix or a block.
#[test]
fn a_held_file_named_after_any_folder_change_is_refused() {
    let fx = Fixture::new("gap-cd-forms");
    write::create_dir_all(&fx.matter().join("Drafts")).unwrap();
    let sub = fx.path("m/sub");
    let commands = [
        "cd Drafts && true || cd /tmp; echo hi > ../Contract.docx".to_owned(),
        "cd Drafts; false && cd /tmp; echo hi > ../Contract.docx".to_owned(),
        "cd Drafts; (cd /tmp); echo hi > ../Contract.docx".to_owned(),
        "cd Drafts # ; cd /tmp\necho hi > ../Contract.docx".to_owned(),
        "cd Drafts\ncat <<EOF\ncd /tmp\nEOF\necho hi > ../Contract.docx".to_owned(),
        "(cd Drafts && echo hi > ../Contract.docx)".to_owned(),
        "( cd Drafts && echo hi > ../Contract.docx )".to_owned(),
        "{ cd Drafts; echo hi > ../Contract.docx; }".to_owned(),
        "true & cd Drafts && echo hi > ../Contract.docx".to_owned(),
        "if true; then cd Drafts; fi; echo hi > ../Contract.docx".to_owned(),
        "builtin cd Drafts && echo hi > ../Contract.docx".to_owned(),
        "command cd Drafts && echo hi > ../Contract.docx".to_owned(),
        "CDPATH= cd Drafts && echo hi > ../Contract.docx".to_owned(),
        "! cd Drafts || echo hi > ../Contract.docx".to_owned(),
        format!("cd ~ && cd {sub} && echo hi > ../Contract.docx"),
        "cd && cd Drafts && echo hi > ../Contract.docx".to_owned(),
        "pushd +1 && echo hi > ../Contract.docx".to_owned(),
        "cd - && echo hi > ../Contract.docx".to_owned(),
        "sh -c 'cd Drafts && echo hi > ../Contract.docx'".to_owned(),
        "bash -lc \"cd Drafts; echo hi >../Contract.docx\"".to_owned(),
        "eval 'cd Drafts && echo hi > ../Contract.docx'".to_owned(),
        "x=$(cd Drafts) ; echo hi > ../Contract.docx".to_owned(),
    ];
    for command in &commands {
        assert_refused(&fx, "Bash", &bash(command));
    }
    for command in [
        "if ($true) { cd Drafts }; Set-Content ..\\Contract.docx hi",
        "& { Set-Location Drafts }; Set-Content ..\\Contract.docx hi",
        "pwsh -NoProfile -Command 'Set-Location Drafts; Set-Content ..\\Contract.docx hi'",
    ] {
        assert_refused(&fx, "PowerShell", &bash(command));
    }
}

/// Review round 1: words the scanner used to read whole.
#[test]
fn redirections_continuations_and_ansi_c_quotes_name_a_held_file() {
    let fx = Fixture::new("gap-words");
    for command in [
        "echo hi >Contract.docx&true",
        "cat>Contract.docx<x",
        "echo hi > Con\\\ntract.docx",
        "echo hi > $'Contract.docx'",
        "echo hi > $'Con'tract.docx",
        "bash -c 'echo hi > Contract.docx'",
    ] {
        assert_refused(&fx, "Bash", &bash(command));
    }
}

/// Review round 1: a huge word, or a Write with a huge path, is read in time proportional to its
/// length, well inside the hook's 10 s (measured in the PR body; the bound is loose for debug
/// builds on a loaded runner).
#[test]
fn a_huge_word_or_path_is_read_quickly() {
    let (done, finished) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        let fx = Fixture::new("gap-huge");
        let deep = format!("{}Contract.docx", "Drafts/".repeat(150_000));
        let back = format!("{}Contract.docx", "sub/../".repeat(150_000));
        // Still long once `..` is resolved by its text: the review's slow case.
        let up = format!("{}../Contract.docx", "Drafts/".repeat(150_000));
        for (tool, input) in [
            ("Write", json!({"file_path": up})),
            ("Bash", bash(&format!("echo hi > {up}"))),
            ("Write", json!({"file_path": deep})),
            ("Bash", bash(&format!("echo hi > {deep}"))),
        ] {
            assert_eq!(guard_json(&fx.hook(tool, &input)), "", "{tool}");
        }
        assert_refused(&fx, "Write", &json!({"file_path": back}));
        assert_refused(&fx, "Bash", &bash(&format!("echo hi > {back}")));
        done.send(()).unwrap();
    });
    let outcome = finished.recv_timeout(std::time::Duration::from_secs(5));
    if outcome.is_ok() || outcome == Err(std::sync::mpsc::RecvTimeoutError::Disconnected) {
        // Done, or a failed assertion: joining passes the panic on.
        worker.join().unwrap();
    }
    assert_eq!(outcome, Ok(()), "the guard took over 5 s");
}

/// Review round 1: a symlink in the matter pointing deeper into it. The OS reads `l/..` as the
/// link target's parent, so the guard resolves the path as spelled too.
#[cfg(unix)]
#[test]
fn a_dotdot_through_a_symlink_deeper_into_the_matter_is_refused() {
    let fx = Fixture::new("gap-symlink-deeper");
    let deeper = fx.matter().join("a").join("b");
    write::create_dir_all(&deeper).unwrap();
    for name in ["a/Only.docx", "a/~$Only.docx"] {
        let path = name
            .split('/')
            .fold(fx.matter(), |acc, part| acc.join(part));
        write::write_file(&path, b"x").unwrap();
    }
    write::symlink(&deeper, &fx.matter().join("l")).unwrap();
    let only = deny("a/Only.docx");
    for command in [
        "mkdir -p l/missing && echo hi > l/missing/../../Only.docx",
        "cd -P l && cd .. && echo hi > Only.docx",
        "echo hi > l/../Only.docx",
    ] {
        assert_eq!(
            guard_json(&fx.hook("Bash", &bash(command))),
            only,
            "{command}"
        );
    }
    assert_eq!(
        guard_json(&fx.hook("Write", &json!({"file_path": "l/missing/../../Only.docx"}))),
        only
    );
}
