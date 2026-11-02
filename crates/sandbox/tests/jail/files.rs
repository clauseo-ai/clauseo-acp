//! Files in the jail, through system tools: the credential link and Claude store, every `.git`
//! and protected name read-only (in any case), what must not be read, links, and the rule
//! order (Seatbelt's later matching rule wins).

use crate::support::{World, launcher, run, sh, tree};

/// A shell loop that appends to each path given and prints `WROTE <path>` when it could.
const APPEND_EACH: &str = r#"for f in "$@"; do printf x >> "$f" && echo "WROTE $f"; done; exit 0"#;

/// Makes each path given (folders first for nested names) and prints `MADE <path>` when it could.
const CREATE_EACH: &str = r#"for f in "$@"; do d=$(dirname "$f"); mkdir -p "$d" 2>/dev/null; printf x > "$f" && echo "MADE $f"; done; exit 0"#;

#[test]
fn the_credential_link_and_the_claude_store_take_appends() {
    let w = World::new();
    let policy = w.builder().build().unwrap();
    let link = policy.credential_link().unwrap().to_owned();
    let store = policy.claude_store().unwrap().files()[0].clone();
    let (launcher, _) = launcher(policy);
    let m1 = w.at("m1");
    let out = sh(
        &launcher,
        &m1,
        APPEND_EACH,
        &[link.to_str().unwrap(), store.to_str().unwrap()],
    );
    out.assert_ok();
    assert!(
        out.stdout.contains(&format!("WROTE {}", link.display())),
        "{out:#?}"
    );
    assert!(
        out.stdout.contains(&format!("WROTE {}", store.display())),
        "{out:#?}"
    );
    // The link is a hard link: the lawyer's login file took the append.
    assert!(
        std::fs::read_to_string(w.at("login/auth.json"))
            .unwrap()
            .ends_with('x')
    );
    // A lock folder in the store can be made.
    let lock = w.at("claude/.oauth_refresh.lock");
    sh(&launcher, &m1, r#"mkdir "$1""#, &[lock.to_str().unwrap()]).assert_ok();
    assert!(lock.is_dir());
}

#[test]
fn a_write_in_the_matter_succeeds() {
    let w = World::new();
    let (launcher, _) = launcher(w.builder().build().unwrap());
    let m1 = w.at("m1");
    sh(
        &launcher,
        &m1,
        "mkdir -p sub && printf ok > sub/new.txt",
        &[],
    )
    .assert_ok();
    assert_eq!(
        std::fs::read_to_string(m1.join("sub/new.txt")).unwrap(),
        "ok"
    );
}

#[test]
fn every_git_file_and_a_gitdir_folder_refuse_writes() {
    let w = World::new();
    w.file("m1/.git/HEAD", "ref: refs/heads/main\n");
    w.file("m1/.git/config", "[core]\n");
    w.file("m1/.git/objects/ab/cdef", "object\n");
    w.file("m1/.git/refs/heads/main", "0000\n");
    w.file("m1/sub/.git/HEAD", "ref: refs/heads/main\n");
    // A worktree-style `.git` file naming a folder inside the matter.
    w.file("m1/wt/.git", "gitdir: ../gd\n");
    w.file("m1/gd/HEAD", "ref: refs/heads/other\n");
    let (launcher, _) = launcher(w.builder().build().unwrap());
    let m1 = w.at("m1");
    let before = tree(&m1);
    let mut targets: Vec<String> = before
        .keys()
        .filter(|p| p.components().any(|c| c.as_os_str() == ".git") || p.starts_with(w.at("m1/gd")))
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    targets.push(w.at("m1/.git/new-file").to_string_lossy().into_owned());
    targets.push(w.at("m1/gd/new-file").to_string_lossy().into_owned());
    let refs: Vec<&str> = targets.iter().map(String::as_str).collect();
    let out = sh(&launcher, &m1, APPEND_EACH, &refs);
    assert!(!out.stdout.contains("WROTE"), "{out:#?}");
    assert_eq!(
        out.stderr.matches("Operation not permitted").count(),
        targets.len(),
        "{out:#?}"
    );
    // Moving a folder that holds a .git would carry it out of its rule.
    sh(&launcher, &m1, "mv sub sub2", &[]).assert_refused();
    sh(&launcher, &m1, "mv .git git2", &[]).assert_refused();
    assert_eq!(tree(&m1), before);
}

#[test]
fn protected_names_cannot_be_created_in_any_case() {
    let w = World::new();
    w.file("m1/.git/config", "[core]\n");
    let big = w.file("m1/big.pdf", "large\n");
    let (launcher, _) = launcher(w.builder().over_cap([big]).build().unwrap());
    let m1 = w.at("m1");
    let names = [
        ".codex/config.toml",
        ".claude/settings.json",
        ".agents/x",
        ".mcp.json",
        "CLAUDE.md",
        "AGENTS.md",
        ".clauseo/matter.json",
        // Case variants: macOS volumes are usually case-insensitive.
        ".CODEX/config.toml",
        ".Claude/settings.json",
        "claude.md",
        "Agents.md",
        ".MCP.json",
        ".GIT/config",
        "BIG.pdf",
    ];
    let out = sh(&launcher, &m1, CREATE_EACH, &names);
    assert!(!out.stdout.contains("MADE"), "{out:#?}");
    for name in names {
        let top = name.split('/').next().unwrap();
        let exists_already = [".GIT", "BIG.pdf"].contains(&top);
        assert!(exists_already || !m1.join(top).exists(), "{name} was made");
    }
    assert_eq!(
        std::fs::read_to_string(m1.join(".git/config")).unwrap(),
        "[core]\n"
    );
    assert_eq!(
        std::fs::read_to_string(m1.join("big.pdf")).unwrap(),
        "large\n"
    );
}

#[test]
fn reads_outside_the_policy_fail() {
    let w = World::new();
    let (launcher, _) = launcher(w.builder().build().unwrap());
    let m1 = w.at("m1");
    for rel in [
        "m2/brief.txt",
        "data/agent-state/m2/codex/history.txt",
        "data/agent-state/m2/notes/n.txt",
        "data/versions/v1.txt",
        "home/secret.txt",
        "login/other.txt",
        "login/auth.json",
        "claude/other.txt",
        "outside/secret.txt",
        // A case variant of a hidden path.
        "DATA/versions/v1.txt",
    ] {
        let path = w.at(rel);
        let out = run(&launcher, "/bin/cat", &[path.to_str().unwrap()], &m1, &[]);
        out.assert_refused();
        assert!(!out.stdout.contains("stand-in"), "{rel}: {out:#?}");
    }
    // The Claude store's folder cannot be listed.
    run(
        &launcher,
        "/bin/ls",
        &[w.at("claude").to_str().unwrap()],
        &m1,
        &[],
    )
    .assert_refused();
}

#[test]
fn links_do_not_lead_outside() {
    let w = World::new();
    w.symlink("outside/secret.txt", "m1/sym-secret");
    w.file("outside/hard.txt", "hard secret\n");
    w.hard_link("outside/hard.txt", "m1/hard-secret");
    let (launcher, _) = launcher(w.builder().build().unwrap());
    let m1 = w.at("m1");
    for name in ["sym-secret", "hard-secret"] {
        let out = run(&launcher, "/bin/cat", &[name], &m1, &[]);
        out.assert_refused();
        assert!(!out.stdout.contains("secret"), "{name}: {out:#?}");
    }
    // A hard link made from inside the jail.
    let outside = w.at("outside/secret.txt");
    run(
        &launcher,
        "/bin/ln",
        &[outside.to_str().unwrap(), "new-link"],
        &m1,
        &[],
    )
    .assert_refused();
    assert!(!m1.join("new-link").exists());
}

#[test]
fn later_rules_win() {
    let w = World::new();
    // A runtime file inside the hidden data dir: the allow after the hidden paths wins.
    let runtime = w.file("data/agents/tool/readme.txt", "runtime\n");
    w.file("data/agents/other.txt", "hidden\n");
    let policy = w
        .builder()
        .runtime(w.at("data/agents/tool"))
        .build()
        .unwrap();
    let (launcher, _) = launcher(policy);
    let m1 = w.at("m1");
    let out = run(
        &launcher,
        "/bin/cat",
        &[runtime.to_str().unwrap()],
        &m1,
        &[],
    );
    assert_eq!(out.assert_ok().stdout, "runtime\n");
    let other = w.at("data/agents/other.txt");
    run(&launcher, "/bin/cat", &[other.to_str().unwrap()], &m1, &[]).assert_refused();
    // Read-only after read-write: a protected name in the writable matter.
    sh(&launcher, &m1, "printf x > AGENTS.md", &[]).assert_refused();
}

#[test]
fn notes_of_another_matter_cannot_be_read() {
    let w = World::new();
    let (launcher, _) = launcher(w.builder().build().unwrap());
    let m1 = w.at("m1");
    let other = w.at("data/agent-state/m2/notes/n.txt");
    run(&launcher, "/bin/cat", &[other.to_str().unwrap()], &m1, &[]).assert_refused();
    let own = w.at("data/agent-state/m1/notes/mine.txt");
    sh(
        &launcher,
        &m1,
        r#"printf n > "$1" && cat "$1""#,
        &[own.to_str().unwrap()],
    )
    .assert_ok();
}
