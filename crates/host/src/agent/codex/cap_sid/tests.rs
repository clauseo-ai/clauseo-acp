use super::foreign;

/// The SIDs a Windows user holds, as `own::sids` gives them: the user's, then its groups'
/// (well-known ones and one of its domain's).
fn held() -> Vec<String> {
    [
        "S-1-5-21-1004336348-1177238915-682003330-1001",
        "S-1-1-0",
        "S-1-5-32-545",
        "S-1-5-11",
        "S-1-5-21-1004336348-1177238915-682003330-513",
    ]
    .map(str::to_owned)
    .to_vec()
}

/// `cap_sid` text with `workspace`, `readonly` and one SID in each map.
fn caps(workspace: &str, readonly: &str, by_cwd: &str, by_root: &str) -> String {
    serde_json::json!({
        "workspace": workspace,
        "readonly": readonly,
        "workspace_by_cwd": {"c:/matter": by_cwd},
        "writable_root_by_path": {"c:/data/run/s1": by_root},
    })
    .to_string()
}

/// What Codex makes is kept: random `S-1-5-21-` SIDs with four numbers, as JSON or as the
/// older one-line form; and text Codex writes new SIDs over (empty, an object it can't read).
#[test]
fn codex_made_sids_are_kept() {
    let made = caps(
        "S-1-5-21-3922318142-1240498823-2788036105-3021407425",
        "S-1-5-21-12-34-56-78",
        "S-1-5-21-1-2-3-4",
        "S-1-5-21-4294967295-0-7-9",
    );
    assert!(!foreign(&made, &held()));
    assert!(!foreign("S-1-5-21-1-2-3-4\n", &held()));
    assert!(!foreign("", &held()));
    assert!(!foreign("{\"workspace\": 3}", &held()));
}

/// A SID the user holds, or of its machine or domain, or of another shape, in any place of the
/// file, is foreign: it would widen what a sandboxed command may write.
#[test]
fn a_sid_the_user_holds_or_codex_could_not_make_is_foreign() {
    let codex = "S-1-5-21-1-2-3-4";
    let planted = [
        "S-1-5-21-1004336348-1177238915-682003330-1001",
        "S-1-5-21-1004336348-1177238915-682003330-1002",
        "S-1-5-21-1004336348-1177238915-682003330-500",
        "S-1-1-0",
        "S-1-5-18",
        "S-1-5-32-544",
        "S-1-5-11",
        "s-1-5-21-1-2-3-4",
        "S-1-5-21-01-2-3-4",
        "S-1-5-21-1-2-3-4-5",
        "S-1-5-21-+1-2-3-4",
    ];
    for sid in planted {
        for text in [
            caps(sid, codex, codex, codex),
            caps(codex, sid, codex, codex),
            caps(codex, codex, sid, codex),
            caps(codex, codex, codex, sid),
            sid.to_owned(),
        ] {
            assert!(foreign(&text, &held()), "{text}");
        }
    }
}
