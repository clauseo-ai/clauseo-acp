use super::*;

const HEADER: &str = r#"{"scenario":"t","description":"test"}"#;

fn parse_text(lines: &[&str], common: &[(&str, &str)]) -> Result<Scenario, Error> {
    let mut text = String::from(HEADER);
    for line in lines {
        text.push('\n');
        text.push_str(line);
    }
    parse("t", &text, &Includes::Memory(common))
}

/// The `file:line: message` text of a load error.
fn load_error(lines: &[&str], common: &[(&str, &str)]) -> String {
    match parse_text(lines, common) {
        Ok(_) => panic!("loaded: {lines:?}"),
        Err(error) => error.to_string(),
    }
}

const INIT: &str = r#"{"expect":{"method":"initialize","as":"init"}}"#;

#[test]
fn include_cannot_nest() {
    let inner = r#"{"include":"inner.jsonl"}"#;
    let common = [("outer.jsonl", inner), ("inner.jsonl", INIT)];
    assert_eq!(
        load_error(&[r#"{"include":"outer.jsonl"}"#], &common),
        "outer.jsonl:1: an included file may not include; included from t.jsonl:2"
    );
    let ok = parse_text(&[r#"{"include":"inner.jsonl"}"#], &common).unwrap();
    assert_eq!(ok.steps.len(), 1);
    assert_eq!(
        ok.steps[0].at,
        Loc {
            included_from: Some(Box::new(Loc::new("t.jsonl", 2))),
            ..Loc::new("inner.jsonl", 1)
        }
    );
    assert_eq!(
        load_error(&[r#"{"include":"../x.jsonl"}"#], &common),
        "t.jsonl:2: include `../x.jsonl` must be a .jsonl file name in scenarios/common/"
    );
    assert_eq!(
        load_error(&[r#"{"include":"missing.jsonl"}"#], &common),
        "t.jsonl:2: include `missing.jsonl`: no such file in scenarios/common/"
    );
}

/// An error inside an included file names that file and line, and the `include` step too, since
/// what is wrong may depend on the file that includes it.
#[test]
fn included_error_names_the_include_step() {
    let broken = [(
        "bad.jsonl",
        "\n{\"respond\":{\"to\":\"nobody\",\"result\":{}}}",
    )];
    assert_eq!(
        load_error(&[INIT, r#"{"include":"bad.jsonl"}"#], &broken),
        "bad.jsonl:2: respond.to `nobody` names no earlier expect label; included from t.jsonl:3"
    );
    let not_json = [("bad.jsonl", "{")];
    assert!(
        load_error(&[r#"{"include":"bad.jsonl"}"#], &not_json)
            .ends_with("; included from t.jsonl:2")
    );
    // The same file loads when the including file has the label it answers.
    let answer = [(
        "answer.jsonl",
        r#"{"respond":{"to":"init","result":{"protocolVersion":1}}}"#,
    )];
    assert!(parse_text(&[INIT, r#"{"include":"answer.jsonl"}"#], &answer).is_ok());
}

#[test]
fn path_outside_cwd_is_load_error() {
    for (step, message) in [
        (
            r#"{"write_file":{"path":"/etc/x","content":""}}"#,
            "path `/etc/x` is absolute",
        ),
        (
            r#"{"write_file":{"path":"C:\\x","content":""}}"#,
            "path `C:\\x` is absolute",
        ),
        (
            r#"{"write_file":{"path":"\\\\srv\\x","content":""}}"#,
            "is absolute",
        ),
        (
            r#"{"delete_file":{"path":"a/../../x"}}"#,
            "has a `..` component",
        ),
        (
            r#"{"move_file":{"from":"a","to":"..\\x"}}"#,
            "has a `..` component",
        ),
        (
            r#"{"spawn_child":{"pid_file":"/tmp/p"}}"#,
            "path `/tmp/p` is absolute; it must be relative to the process folder",
        ),
        (
            r#"{"spawn_child":{"pid_file":"../p"}}"#,
            "it must stay under the process folder (the agent's working directory)",
        ),
        (
            r#"{"write_file":{"path":"/x","content":""}}"#,
            "path `/x` is absolute; it must be relative to the session cwd",
        ),
        (r#"{"delete_file":{"path":""}}"#, "the path is empty"),
    ] {
        let error = load_error(&[step], &[]);
        assert!(error.starts_with("t.jsonl:2: "), "{error}");
        assert!(error.contains(message), "{step}: {error}");
    }
    let fine = r#"{"write_file":{"path":"notes/a..b/c.txt","content":""}}"#;
    assert!(parse_text(&[fine], &[]).is_ok());
}

#[test]
fn static_checks_name_file_and_line() {
    let new = r#"{"expect":{"method":"session/new","as":"init"}}"#;
    let cases: &[(&[&str], &str)] = &[
        (&[INIT, new], "t.jsonl:3: label `init` is used twice"),
        (
            &[r#"{"respond":{"to":"init","result":{}}}"#],
            "t.jsonl:2: respond.to `init` names no earlier expect label",
        ),
        (
            &[r#"{"expect_response":{"id":"perm-1","result":{}}}"#],
            "t.jsonl:2: expect_response.id `perm-1` names no earlier request step",
        ),
        (
            &[r#"{"expect":{"method":"initialize"}}"#],
            "t.jsonl:2: expect `initialize`: a request needs an `as` label",
        ),
        (
            &[r#"{"expect":{"method":"session/cancel","as":"c"}}"#],
            "t.jsonl:2: expect `session/cancel`: a notification takes no `as` label",
        ),
        (
            &[r#"{"note":"only a note"}"#],
            "t.jsonl:2: a step needs one of: expect,",
        ),
        (
            &[r#"{"expect":{"method":"initialize","as":"i"},"exit":{"code":1}}"#],
            "t.jsonl:2: a step has exactly one step key, found `exit` and `expect`",
        ),
        (
            &[r#"{"expect":{"method":"initialize","as":"i","param":{}}}"#],
            "t.jsonl:2: expect: unknown field `param`",
        ),
        (
            &[r#"{"wait":{}}"#],
            "t.jsonl:2: unknown step `wait`; expected one of: expect,",
        ),
        (&["{not json"], "t.jsonl:2: not JSON"),
        (
            &[
                INIT,
                r#"{"respond":{"to":"init","result":{"protocolVersion":1}}}"#,
                r#"{"respond":{"to":"init","result":{"protocolVersion":1}}}"#,
            ],
            "t.jsonl:4: respond.to `init`: already answered",
        ),
        (
            &[
                INIT,
                r#"{"respond":{"to":"init","result":{"protocolVersion":"one"}}}"#,
            ],
            "t.jsonl:3: initialize: /result: ",
        ),
        (
            &[r#"{"notify":{"method":"_x/y","params":{}}}"#],
            "t.jsonl:2: `_x/y` has no type in the SDK's stable schema; mark the step \"raw\": true",
        ),
        (
            &[
                r#"{"all":[{"expect":{"method":"initialize","as":"a"}},{"respond":{"to":"a","result":{}}}]}"#,
            ],
            "t.jsonl:2: all: cannot wait on `respond`; expected one of: expect, expect_response",
        ),
    ];
    for (lines, want) in cases {
        let error = load_error(lines, &[]);
        assert!(error.starts_with(want), "want {want}\n got {error}");
    }
    let raw = r#"{"notify":{"method":"_x/y","params":{},"raw":true}}"#;
    assert!(parse_text(&[raw], &[]).is_ok());
}

/// Every step key, in the order `body.rs` declares them.
const KEYS: &str = "expect, expect_response, all, respond, update, notify, request, send_line, \
                    write_file, delete_file, move_file, stderr, spawn_child, exit, include";

#[test]
fn a_wrong_step_key_lists_every_key() {
    assert_eq!(
        load_error(&[r#"{"wait":{}}"#], &[]),
        format!("t.jsonl:2: unknown step `wait`; expected one of: {KEYS}")
    );
    assert_eq!(
        load_error(
            &[r#"{"note":"n","expect":{"method":"initialize","as":"i"},"exit":{"code":0}}"#],
            &[]
        ),
        format!(
            "t.jsonl:2: a step has exactly one step key, found `exit` and `expect`; expected \
             one of: {KEYS}"
        )
    );
    assert_eq!(
        load_error(&[r#"{"note":"only a note"}"#], &[]),
        format!("t.jsonl:2: a step needs one of: {KEYS}")
    );
    assert_eq!(
        load_error(&[r#"{"respond":{"to":"a","result":{},"raw":1}}"#], &[]),
        "t.jsonl:2: respond: invalid type: integer `1`, expected a boolean"
    );
}

#[test]
fn exit_steps_may_not_use_the_load_or_mismatch_code() {
    for (code, name) in [
        (crate::EXIT_LOAD, "EXIT_LOAD"),
        (crate::EXIT_MISMATCH, "EXIT_MISMATCH"),
    ] {
        let step = format!(r#"{{"exit":{{"code":{code}}}}}"#);
        assert_eq!(
            load_error(&[&step], &[]),
            format!(
                "t.jsonl:2: exit: code {code} is {name}, the code fake_agent itself exits with; an \
                 exit step may not use 2 (EXIT_LOAD) or 3 (EXIT_MISMATCH)"
            )
        );
    }
}

#[test]
fn stderr_repeat_is_capped() {
    let at_cap = format!(
        r#"{{"stderr":{{"line":"x","repeat":{}}}}}"#,
        body::MAX_REPEAT
    );
    assert!(parse_text(&[&at_cap], &[]).is_ok());
    let over = format!(
        r#"{{"stderr":{{"line":"x","repeat":{}}}}}"#,
        body::MAX_REPEAT + 1
    );
    assert_eq!(
        load_error(&[&over], &[]),
        "t.jsonl:2: stderr: repeat 100001 is over the cap of 100000"
    );
}

#[test]
fn send_line_refuses_cwd() {
    let error = load_error(&[r#"{"send_line":"{\"path\":\"{{cwd}}\"}"}"#], &[]);
    assert!(
        error.starts_with("t.jsonl:2: send_line: `{{cwd}}` is not allowed"),
        "{error}"
    );
    assert!(parse_text(&[r#"{"send_line":"{\"path\":\"cwd\"}"}"#], &[]).is_ok());
}

#[test]
fn exit_code_outside_a_byte_is_load_error() {
    for code in ["256", "-1", "4294967296"] {
        let step = format!(r#"{{"exit":{{"code":{code}}}}}"#);
        assert_eq!(
            load_error(&[&step], &[]),
            format!("t.jsonl:2: exit: code {code} is outside 0..=255")
        );
    }
    for code in ["0", "1", "4", "255"] {
        let step = format!(r#"{{"exit":{{"code":{code}}}}}"#);
        assert!(parse_text(&[&step], &[]).is_ok(), "{code}");
    }
}

#[test]
fn labels_in_all_count_after_the_all_step() {
    let request = r#"{"request":{"id":"perm-1","method":"session/request_permission","raw":true}}"#;
    let prompt = r#"{"expect":{"method":"session/prompt","as":"p1"}}"#;
    let all = r#"{"all":[{"expect":{"method":"session/cancel"}},{"expect_response":{"id":"perm-1","result":{}}}]}"#;
    let respond = r#"{"respond":{"to":"p1","result":{"stopReason":"cancelled"}}}"#;
    assert!(parse_text(&[prompt, request, all, respond], &[]).is_ok());
    let inner = r#"{"all":[{"expect":{"method":"session/prompt","as":"p2"}},{"expect":{"method":"session/cancel"}}]}"#;
    let answer = r#"{"respond":{"to":"p2","result":{"stopReason":"end_turn"}}}"#;
    assert!(parse_text(&[inner, answer], &[]).is_ok());
}

#[test]
fn header_is_checked() {
    let parse_header =
        |header: &str| parse("t", header, &Includes::Memory(&[])).map(|s| s.on_stdin_close);
    assert_eq!(parse_header(HEADER).unwrap(), OnStdinClose::Exit);
    let hang = r#"{"scenario":"t","description":"d","on_stdin_close":"hang"}"#;
    assert_eq!(parse_header(hang).unwrap(), OnStdinClose::Hang);
    let other = r#"{"scenario":"u","description":"d"}"#;
    assert!(
        parse_header(other)
            .unwrap_err()
            .to_string()
            .contains("differs from the file stem")
    );
    assert!(
        parse_header("")
            .unwrap_err()
            .to_string()
            .starts_with("t.jsonl:1: empty file")
    );
    let bad = r#"{"scenario":"t","description":"d","on_stdin_close":"wait"}"#;
    assert_eq!(
        parse_header(bad).unwrap_err().to_string(),
        "t.jsonl:1: header: unknown variant `wait`, expected `exit` or `hang`"
    );
    let extra = r#"{"scenario":"t","description":"d","x":1}"#;
    assert!(
        parse_header(extra)
            .unwrap_err()
            .to_string()
            .contains("unknown field `x`")
    );
}

#[test]
fn stderr_repeat_defaults_to_one() {
    let scenario = parse_text(&[r#"{"stderr":{"line":"x"}}"#], &[]).unwrap();
    assert!(matches!(
        scenario.steps[0].kind,
        StepKind::Stderr { repeat: 1, .. }
    ));
}
