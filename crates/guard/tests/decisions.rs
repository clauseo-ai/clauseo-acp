//! The sidecar decides exactly as the desktop's guard mode: each hook input of the corpus goes
//! through this binary (a real process, over pipes) and through
//! `clauseo_host::pre_write_guard::run`, which `main_entry` (the desktop binary's guard mode
//! and this binary's `main`) calls on the process's stdin and stderr; stdout, stderr and the exit
//! code must match byte for byte. Each case also says what the decision is, so a corpus that
//! stopped refusing anything fails.
//!
//! The corpus: the agents' captured input shapes (`crates/host/tests/fixtures/pre_write_guard`)
//! on a held and a free file, Claude Code's file tools and shell commands, Codex's `.codex` rule,
//! and input the guard ignores. `tests/matter` is the matter folder: `Contract.docx` held open
//! (its Word owner file `~$ntract.docx` beside it) and `Free.docx`.
//!
//! The tests sit in a `#[cfg(test)]` module so clippy treats their helpers as test code.

#[cfg(test)]
mod decisions {
    use std::ffi::OsString;
    use std::path::PathBuf;

    use async_process::{Command, Stdio};
    use clauseo_host::pre_write_guard::{DENY, FLAG, IGNORED, MATTER_FLAG, SIDECAR, run};
    use futures::AsyncWriteExt as _;

    const GUARD: &str = env!("CARGO_BIN_EXE_clauseo-guard");

    /// What the guard decides for a case.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Decision {
        /// Exit 2 with the reason.
        Refuse,
        /// Exit 0, nothing written.
        Allow,
        /// Exit 0 with an [`IGNORED`] line.
        Ignore,
    }

    /// One run's outcome.
    #[derive(Debug, PartialEq, Eq)]
    struct Outcome {
        code: Option<i32>,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
    }

    impl Outcome {
        fn decision(&self) -> Option<Decision> {
            match (self.code, self.stderr.is_empty()) {
                (Some(DENY), false) => Some(Decision::Refuse),
                (Some(0), true) => Some(Decision::Allow),
                (Some(0), false) if self.stderr.starts_with(IGNORED.as_bytes()) => {
                    Some(Decision::Ignore)
                }
                _ => None,
            }
        }
    }

    /// One hook call: the arguments after [`FLAG`] and the input on stdin.
    struct Case {
        name: String,
        args: Vec<OsString>,
        input: Vec<u8>,
        decision: Decision,
    }

    fn matter() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("matter")
    }

    fn matter_args() -> Vec<OsString> {
        vec![MATTER_FLAG.into(), matter().into_os_string()]
    }

    /// A Claude Code hook input for `tool` with `input`, the matter folder as `cwd`.
    fn claude(tool: &str, input: &serde_json::Value) -> Vec<u8> {
        serde_json::json!({
            "session_id": "s-1",
            "transcript_path": "/nowhere/t.jsonl",
            "cwd": matter(),
            "permission_mode": "bypassPermissions",
            "hook_event_name": "PreToolUse",
            "tool_name": tool,
            "tool_input": input,
            "tool_use_id": "toolu_1",
        })
        .to_string()
        .into_bytes()
    }

    /// The captured shapes: (file under the host's fixtures, whether its hook passes `--matter`).
    const SHAPES: [(&str, bool); 8] = [
        ("claude/Write.json", true),
        ("claude/Edit.json", true),
        ("claude/MultiEdit.json", true),
        ("claude/NotebookEdit.json", true),
        ("claude/Bash.json", true),
        ("claude/PowerShell.json", true),
        ("codex/apply_patch.json", false),
        ("codex/Bash.json", false),
    ];

    fn corpus() -> Vec<Case> {
        let mut cases = Vec::new();
        let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../host/tests/fixtures/pre_write_guard");
        // The folder as a JSON string's content: Windows' backslashes escaped.
        let escaped = serde_json::to_string(&matter()).unwrap();
        let escaped = escaped.trim_matches('"');
        for (file, with_matter) in SHAPES {
            let text = std::fs::read_to_string(fixtures.join(file)).unwrap();
            let held = text.replace("{matter}", escaped);
            assert_ne!(held, text, "{file} names no {{matter}}");
            let free = held.replace("Contract.docx", "Free.docx");
            let args = if with_matter {
                matter_args()
            } else {
                Vec::new()
            };
            for (input, decision, which) in [
                (held, Decision::Refuse, "held"),
                (free, Decision::Allow, "free"),
            ] {
                cases.push(Case {
                    name: format!("{file} ({which})"),
                    args: args.clone(),
                    input: input.into_bytes(),
                    decision,
                });
            }
        }
        let contract = matter().join("Contract.docx");
        let free = matter().join("Free.docx");
        let built = [
            (
                "Write held",
                claude(
                    "Write",
                    &serde_json::json!({"file_path": contract, "content": "new"}),
                ),
                Decision::Refuse,
            ),
            (
                "Edit free",
                claude(
                    "Edit",
                    &serde_json::json!({"file_path": free, "old_string": "a", "new_string": "b"}),
                ),
                Decision::Allow,
            ),
            (
                "MultiEdit relative held",
                claude(
                    "MultiEdit",
                    &serde_json::json!({"file_path": "Contract.docx", "edits": []}),
                ),
                Decision::Refuse,
            ),
            (
                "Bash rm held",
                claude(
                    "Bash",
                    &serde_json::json!({"command": "rm Contract.docx; ls"}),
                ),
                Decision::Refuse,
            ),
            (
                "Bash read held",
                claude(
                    "Bash",
                    &serde_json::json!({"command": "cat Contract.docx | grep x > notes.txt"}),
                ),
                Decision::Allow,
            ),
            (
                "Bash wildcard",
                claude(
                    "Bash",
                    &serde_json::json!({"command": "cp Revised/*.docx ."}),
                ),
                Decision::Refuse,
            ),
            (
                "Read is not checked",
                claude("Read", &serde_json::json!({"file_path": contract})),
                Decision::Allow,
            ),
            (
                "Write without file_path",
                claude("Write", &serde_json::json!({"content": "new"})),
                Decision::Ignore,
            ),
        ];
        for (name, input, decision) in built {
            cases.push(Case {
                name: name.to_owned(),
                args: matter_args(),
                input,
                decision,
            });
        }
        // Codex's hook: no --matter, the matter folder as cwd.
        let codex_patch = serde_json::json!({
            "cwd": matter(),
            "hook_event_name": "PreToolUse",
            "tool_name": "apply_patch",
            "tool_input": {"command": "*** Begin Patch\n*** Add File: .codex/config.toml\n+x\n*** End Patch\n"},
        });
        cases.push(Case {
            name: "Codex patch into .codex".to_owned(),
            args: Vec::new(),
            input: codex_patch.to_string().into_bytes(),
            decision: Decision::Refuse,
        });
        let unreadable: [(&str, &[u8]); 5] = [
            ("empty", b""),
            ("whitespace", b"  \n"),
            ("not JSON", b"not json"),
            ("cut short", b"{\"tool_name\": \"Write\", \"tool_in"),
            (
                "no cwd",
                b"{\"tool_name\": \"Write\", \"tool_input\": {\"file_path\": \"x\"}}",
            ),
        ];
        for (name, input) in unreadable {
            cases.push(Case {
                name: name.to_owned(),
                args: Vec::new(),
                input: input.to_vec(),
                decision: Decision::Ignore,
            });
        }
        // A relative --matter is no folder: the input's cwd stands in.
        cases.push(Case {
            name: "relative --matter".to_owned(),
            args: vec![MATTER_FLAG.into(), "matter".into()],
            input: claude("Write", &serde_json::json!({"file_path": contract})),
            decision: Decision::Refuse,
        });
        cases
    }

    /// The case through `run`, as the desktop's guard mode runs it (no stdout at all).
    fn in_process(case: &Case) -> Outcome {
        let mut stderr = Vec::new();
        let code = run(&case.args, case.input.as_slice(), &mut stderr);
        Outcome {
            code: Some(code),
            stdout: Vec::new(),
            stderr,
        }
    }

    /// The case through the sidecar binary: `--pre-write-guard`, the arguments, the input on stdin.
    async fn sidecar(case: &Case) -> Outcome {
        let mut child = Command::new(GUARD)
            .arg(FLAG)
            .args(&case.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        stdin.write_all(&case.input).await.unwrap();
        stdin.close().await.unwrap();
        drop(stdin);
        let output = child.output().await.unwrap();
        Outcome {
            code: output.status.code(),
            stdout: output.stdout,
            stderr: output.stderr,
        }
    }

    #[test]
    fn the_sidecar_and_the_guard_mode_decide_each_case_alike() {
        let cases = corpus();
        for case in &cases {
            let expected = in_process(case);
            assert_eq!(
                expected.decision(),
                Some(case.decision),
                "{}: {:?}",
                case.name,
                String::from_utf8_lossy(&expected.stderr)
            );
            let got = futures::executor::block_on(sidecar(case));
            assert_eq!(got, expected, "{}", case.name);
        }
        for decision in [Decision::Refuse, Decision::Allow, Decision::Ignore] {
            assert!(
                cases.iter().any(|case| case.decision == decision),
                "{decision:?}"
            );
        }
    }

    #[test]
    fn any_other_first_argument_prints_the_usage_and_never_refuses() {
        for args in [
            &[][..],
            &["--matter", "/m"],
            &["--pre-write-guard=1"],
            &["--help"],
        ] {
            let output = futures::executor::block_on(async {
                Command::new(GUARD)
                    .args(args)
                    .stdin(Stdio::null())
                    .output()
                    .await
                    .unwrap()
            });
            assert_eq!(output.status.code(), Some(64), "{args:?}");
            assert_eq!(
                String::from_utf8_lossy(&output.stderr),
                "usage: clauseo-guard --pre-write-guard [--matter <folder>]\n"
            );
            assert_eq!(output.stdout, b"");
        }
    }

    /// The host looks for the sidecar under this binary's name.
    #[test]
    fn the_binary_has_the_name_the_host_looks_for() {
        let name = PathBuf::from(GUARD);
        assert_eq!(
            name.file_name().unwrap().to_str().unwrap(),
            format!("{SIDECAR}{}", std::env::consts::EXE_SUFFIX)
        );
    }
}
