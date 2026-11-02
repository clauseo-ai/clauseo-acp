use super::*;
use crate::test_support::TempDir;

/// A matter, a state folder, a home and a data dir side by side in a temp folder.
struct World {
    dir: TempDir,
}

impl World {
    fn new(test: &str) -> World {
        let dir = TempDir::new(test);
        for name in [
            "matter",
            "state",
            "home",
            "data/agent-state/m1/notes",
            "other",
        ] {
            dir.dir(name);
        }
        World { dir }
    }

    fn at(&self, rel: &str) -> PathBuf {
        self.dir.path().join(rel)
    }

    /// The builder every test starts from: this world's matter, state and home.
    fn builder(&self) -> PolicyBuilder {
        Policy::builder(self.at("matter"), self.at("state")).home(self.at("home"))
    }
}

fn canon(p: &Path) -> PathBuf {
    dunce::canonicalize(p).unwrap()
}

/// The refusal's message, which must name `rule` and `path`.
#[track_caller]
fn refused(result: Result<Policy, Error>, rule: &str, path: &Path) {
    match result {
        Err(Error::Policy(message)) => {
            assert!(message.contains(rule), "{message:?} should name {rule:?}");
            let name = path
                .file_name()
                .map_or_else(|| path.to_string_lossy(), |name| name.to_string_lossy());
            assert!(
                message.contains(&*name),
                "{message:?} should name {}",
                path.display()
            );
        }
        other => panic!("expected a refusal naming {rule:?}, got {other:?}"),
    }
}

#[test]
fn a_plain_policy_builds_with_canonical_paths() {
    let w = World::new("plain");
    let policy = w.builder().build().unwrap();
    assert_eq!(policy.matter(), canon(&w.at("matter")));
    assert_eq!(policy.state(), canon(&w.at("state")));
    assert_eq!(policy.tmpdir(), canon(&w.at("state")).join("tmp"));
    assert!(policy.proxy_ports().is_empty(), "no port means no network");
    assert_eq!(policy.loopback(), &Loopback::None);
    assert!(!policy.keychain());
    #[cfg(target_os = "macos")]
    {
        // The temp folder is under /var, which is /private/var.
        assert!(
            w.at("matter").starts_with("/var"),
            "{}",
            w.at("matter").display()
        );
        assert!(policy.matter().starts_with("/private/var"));
    }
}

#[test]
fn relative_paths_are_refused() {
    let w = World::new("relative");
    refused(
        Policy::builder("matter", w.at("state")).build(),
        "must be an absolute path",
        Path::new("matter"),
    );
    refused(
        w.builder().allowed_folder("other").build(),
        "must be an absolute path",
        Path::new("other"),
    );
    refused(
        w.builder().hidden("data").build(),
        "must be an absolute path",
        Path::new("data"),
    );
    refused(
        w.builder().credential_link("auth.json").build(),
        "must be an absolute path",
        Path::new("auth.json"),
    );
}

#[test]
fn a_missing_matter_is_refused() {
    let w = World::new("missing");
    refused(
        Policy::builder(w.at("gone"), w.at("state")).build(),
        "must exist",
        &w.at("gone"),
    );
}

#[test]
fn the_matter_may_not_meet_a_hidden_path() {
    let w = World::new("matter-hidden");
    let rule = "the matter may not equal, hold or sit inside a hidden path";
    refused(
        w.builder().hidden(w.at("matter")).build(),
        rule,
        &w.at("matter"),
    );
    // A matter inside another matter: the outer one is hidden.
    refused(
        w.builder().hidden(w.dir.path()).build(),
        rule,
        &w.at("matter"),
    );
    w.dir.dir("matter/inner");
    refused(
        w.builder().hidden(w.at("matter/inner")).build(),
        rule,
        &w.at("matter"),
    );
}

#[test]
fn the_matter_may_not_be_the_root_or_the_home_folder() {
    let w = World::new("matter-root");
    refused(
        Policy::builder("/", w.at("state"))
            .home(w.at("home"))
            .build(),
        "the matter may not be the root folder",
        Path::new("/"),
    );
    refused(
        Policy::builder(w.at("home"), w.at("state"))
            .home(w.at("home"))
            .build(),
        "the matter may not be the home folder",
        &w.at("home"),
    );
}

#[test]
fn the_state_folder_and_the_matter_may_not_nest() {
    let w = World::new("state-nest");
    let rule = "the state folder and the matter may not be one inside the other";
    w.dir.dir("matter/.state");
    refused(
        Policy::builder(w.at("matter"), w.at("matter/.state"))
            .home(w.at("home"))
            .build(),
        rule,
        &w.at("matter"),
    );
    w.dir.dir("state/matter");
    refused(
        Policy::builder(w.at("state/matter"), w.at("state"))
            .home(w.at("home"))
            .build(),
        rule,
        &w.at("state"),
    );
}

#[test]
fn notes_and_the_credential_link_may_not_be_in_the_matter() {
    let w = World::new("in-matter");
    w.dir.dir("matter/notes");
    refused(
        w.builder().notes(w.at("matter/notes")).build(),
        "the notes folder may not be inside the matter",
        &w.at("matter/notes"),
    );
    refused(
        w.builder()
            .credential_link(w.at("matter/auth.json"))
            .build(),
        "the credential link may not be inside the matter",
        &w.at("matter/auth.json"),
    );
}

#[test]
fn allowed_folders_that_meet_a_hidden_path_are_refused() {
    let w = World::new("allowed-hidden");
    let rule = "an allowed folder may not equal, hold or sit inside a hidden path";
    let data = w.at("data");
    w.dir.dir("data/agents");
    // Equal (the data dir), holds (a folder holding the data dir), inside (a folder in it).
    refused(
        w.builder().hidden(&data).allowed_folder(&data).build(),
        rule,
        &data,
    );
    w.dir.dir("outer/data2");
    refused(
        w.builder()
            .hidden(w.at("outer/data2"))
            .allowed_folder(w.at("outer"))
            .build(),
        rule,
        &w.at("outer"),
    );
    refused(
        w.builder()
            .hidden(&data)
            .allowed_folder(w.at("data/agents"))
            .build(),
        rule,
        &w.at("data/agents"),
    );
    // Another matter and a refused folder are hidden paths too.
    w.dir.dir("matter2");
    refused(
        w.builder()
            .hidden(w.at("matter2"))
            .allowed_folder(w.at("matter2"))
            .build(),
        rule,
        &w.at("matter2"),
    );
}

#[test]
fn allowed_folders_that_meet_the_matter_are_refused() {
    let w = World::new("allowed-matter");
    let rule = "an allowed folder may not equal, hold or sit inside the matter";
    refused(
        w.builder().allowed_folder(w.at("matter")).build(),
        rule,
        &w.at("matter"),
    );
    w.dir.dir("matter/sub");
    refused(
        w.builder().allowed_folder(w.at("matter/sub")).build(),
        rule,
        &w.at("matter/sub"),
    );
    // `Documents/Clauseo`, the parent of new matters, is refused because it holds the matter.
    w.dir.dir("Documents/Clauseo/Smith v Jones");
    refused(
        Policy::builder(w.at("Documents/Clauseo/Smith v Jones"), w.at("state"))
            .home(w.at("home"))
            .allowed_folder(w.at("Documents/Clauseo"))
            .build(),
        rule,
        &w.at("Documents/Clauseo"),
    );
}

#[test]
fn allowed_folders_that_are_home_root_or_git_are_refused() {
    let w = World::new("allowed-home");
    let home_rule = "an allowed folder may not equal or hold the home folder";
    refused(
        w.builder().allowed_folder(w.at("home")).build(),
        home_rule,
        &w.at("home"),
    );
    refused(
        w.builder().allowed_folder(w.dir.path()).build(),
        home_rule,
        w.dir.path(),
    );
    refused(
        w.builder().allowed_folder("/").build(),
        "an allowed folder may not be the root folder",
        Path::new("/"),
    );
    w.dir.dir("other/repo/.git/objects");
    refused(
        w.builder()
            .allowed_folder(w.at("other/repo/.git/objects"))
            .build(),
        "an allowed folder may not be in a .git",
        &w.at("other/repo/.git/objects"),
    );
}

#[test]
fn a_good_allowed_folder_is_kept_and_check_canonicalises_first() {
    let w = World::new("allowed-ok");
    w.dir.dir("home/Precedents");
    let policy = w
        .builder()
        .hidden(w.at("data"))
        .allowed_folder(w.at("home/Precedents"))
        .build()
        .unwrap();
    assert_eq!(policy.allowed_folders(), [canon(&w.at("home/Precedents"))]);
    // `other/../data` is the data dir once resolved.
    let sneaky = w.at("other/../data");
    match policy.check_allowed_folder(&sneaky) {
        Err(Error::Policy(message)) => assert!(message.contains("hidden path"), "{message}"),
        other => panic!("expected a refusal, got {other:?}"),
    }
    assert!(policy.check_allowed_folder(&w.at("other")).is_ok());
    assert!(matches!(
        policy.check_allowed_folder(Path::new("relative")),
        Err(Error::Policy(_))
    ));
}

#[test]
fn protected_names_are_read_only_when_absent() {
    let w = World::new("protected");
    let policy = w.builder().build().unwrap();
    let matter = canon(&w.at("matter"));
    for name in PROTECTED_NAMES {
        let rule = PathRule::Subpath(matter.join(name));
        assert!(
            policy.read_only_in_matter().contains(&rule),
            "{name} missing"
        );
        assert!(!matter.join(name).exists());
    }
    assert_eq!(
        policy.classify(&matter.join(".codex/config.toml"), Access::Write),
        PathClass::ReadOnly
    );
}

#[test]
fn every_git_at_any_depth_is_read_only() {
    let w = World::new("git-depth");
    w.dir.file("matter/.git/HEAD", "ref: refs/heads/main\n");
    w.dir.file("matter/a/b/c/.git/config", "[core]\n");
    let policy = w.builder().build().unwrap();
    let matter = canon(&w.at("matter"));
    let rules = policy.read_only_in_matter();
    assert!(rules.contains(&PathRule::Subpath(matter.join(".git"))));
    assert!(rules.contains(&PathRule::Subpath(matter.join("a/b/c/.git"))));
    assert_eq!(
        policy.classify(&matter.join("a/b/c/.git/config"), Access::Write),
        PathClass::ReadOnly
    );
    assert_eq!(
        policy.classify(&matter.join("a/b/c/.git/config"), Access::Read),
        PathClass::Outside
    );
}

#[test]
fn a_gitdir_file_makes_its_folder_read_only_when_inside_the_matter() {
    let w = World::new("gitdir");
    w.dir.dir("matter/.store/worktrees/wt");
    w.dir
        .file("matter/wt/.git", "gitdir: ../.store/worktrees/wt\n");
    w.dir.dir("other/worktrees/far");
    let far = w.at("other/worktrees/far");
    w.dir
        .file("matter/far/.git", &format!("gitdir: {}\n", far.display()));
    let policy = w.builder().build().unwrap();
    let matter = canon(&w.at("matter"));
    let rules = policy.read_only_in_matter();
    assert!(rules.contains(&PathRule::Subpath(matter.join("wt/.git"))));
    assert!(rules.contains(&PathRule::Subpath(matter.join(".store/worktrees/wt"))));
    assert!(rules.contains(&PathRule::Subpath(matter.join("far/.git"))));
    assert!(
        !rules.iter().any(|r| r.path().starts_with(canon(&far))),
        "outside the matter it is denied anyway: {rules:?}"
    );
}

#[test]
fn over_cap_files_are_literal_and_must_be_in_the_matter() {
    let w = World::new("over-cap");
    let big = w.dir.file("matter/video.mov", "");
    let policy = w.builder().over_cap([big.clone()]).build().unwrap();
    let rule = PathRule::Literal(canon(&big));
    assert!(policy.read_only_in_matter().contains(&rule));
    assert_eq!(policy.over_cap(), [canon(&big)]);
    let outside = w.dir.file("other/video.mov", "");
    refused(
        w.builder().over_cap([outside.clone()]).build(),
        "must be inside",
        &outside,
    );
}

#[cfg(unix)]
#[test]
fn the_credential_link_is_not_resolved() {
    let w = World::new("cred-link");
    let login = w.dir.file("home/.codex/auth.json", "{}");
    let link = w.dir.symlink("state/auth.json", &login);
    let policy = w.builder().credential_link(&link).build().unwrap();
    assert_eq!(
        policy.credential_link(),
        Some(canon(&w.at("state")).join("auth.json").as_path())
    );
    assert_ne!(policy.credential_link(), Some(canon(&login).as_path()));
}

#[test]
fn the_claude_store_allows_only_its_names() {
    let w = World::new("claude-store");
    w.dir.dir("home/.claude");
    let policy = w
        .builder()
        .hidden(w.at("home"))
        .claude_store(w.at("home/.claude"))
        .build()
        .unwrap();
    let store = policy.claude_store().unwrap();
    let dir = canon(&w.at("home/.claude"));
    assert_eq!(store.dir(), dir);
    assert_eq!(
        store.files(),
        [dir.join(".credentials.json"), dir.join(".storage-write")]
    );
    assert_eq!(
        store.lock_folders(),
        [
            dir.join(".storage-write.lock"),
            dir.join(".oauth_refresh.lock")
        ]
    );
    // The store's names are allowed again after the hidden home; a sibling is not.
    let other = dir.join("other.txt");
    assert_eq!(policy.classify(&other, Access::Read), PathClass::Hidden);
    let creds = dir.join(".credentials.json");
    assert_eq!(policy.classify(&creds, Access::Read), PathClass::Outside);
}

#[test]
fn hidden_paths_need_not_exist() {
    let w = World::new("hidden-missing");
    let policy = w
        .builder()
        .hidden(w.at("data/agent-state/m2/notes"))
        .hidden(w.at("not-yet/deeper"))
        .build()
        .unwrap();
    let root = canon(w.dir.path());
    assert_eq!(
        policy.hidden(),
        [
            root.join("data/agent-state/m2/notes"),
            root.join("not-yet/deeper")
        ]
    );
    refused(
        w.builder().hidden(w.at("not-yet/../../x")).build(),
        "`..`",
        Path::new("x"),
    );
}

#[test]
fn read_only_in_state_must_be_in_the_state_folder() {
    let w = World::new("ro-state");
    let settings = w.dir.file("state/settings.json", "{}");
    let policy = w.builder().read_only_in_state(&settings).build().unwrap();
    assert_eq!(policy.read_only_in_state(), [canon(&settings)]);
    assert_eq!(
        policy.classify(&canon(&settings), Access::Write),
        PathClass::ReadOnly
    );
    let elsewhere = w.dir.file("other/settings.json", "{}");
    refused(
        w.builder().read_only_in_state(&elsewhere).build(),
        "must be inside",
        &elsewhere,
    );
}

#[test]
fn ports_are_sorted_and_deduplicated() {
    let w = World::new("ports");
    let policy = w
        .builder()
        .proxy_port(61002)
        .proxy_port(61001)
        .proxy_port(61002)
        .loopback(Loopback::Ports(vec![9300, 9222, 9300]))
        .keychain(true)
        .build()
        .unwrap();
    assert_eq!(policy.proxy_ports(), [61001, 61002]);
    assert_eq!(policy.loopback(), &Loopback::Ports(vec![9222, 9300]));
    assert!(policy.keychain());
}

#[test]
fn classify_orders_link_hidden_read_only_outside() {
    let w = World::new("classify");
    w.dir.dir("data/agents/claude");
    w.dir.dir("data/agent-state/m2");
    let target = w.dir.file("other/secret.txt", "s");
    let policy = w
        .builder()
        .hidden(w.at("data"))
        .runtime(w.at("data/agents/claude"))
        .notes(w.at("data/agent-state/m1/notes"))
        .links([LinkFinding {
            path: canon(&w.at("matter")).join("secret.txt"),
            kind: crate::LinkKind::Symlink,
            target: Some(canon(&target)),
        }])
        .build()
        .unwrap();
    let data = canon(&w.at("data"));
    let matter = canon(&w.at("matter"));
    let class = |p: &Path, a| policy.classify(p, a);
    assert_eq!(class(&canon(&target), Access::Read), PathClass::LinkTarget);
    assert_eq!(
        class(&matter.join("secret.txt"), Access::Write),
        PathClass::LinkTarget
    );
    assert_eq!(
        class(&data.join("agent-state/m2/x"), Access::Read),
        PathClass::Hidden
    );
    assert_eq!(
        class(&data.join("agents/claude/cli.js"), Access::Write),
        PathClass::ReadOnly,
        "the runtime is allowed again after the hidden data dir, read-only"
    );
    assert_eq!(
        class(&data.join("agent-state/m1/notes/a.md"), Access::Read),
        PathClass::Outside
    );
    assert_eq!(
        class(&matter.join(".claude/settings.json"), Access::Write),
        PathClass::ReadOnly
    );
    assert_eq!(
        class(&canon(&w.at("other")), Access::Read),
        PathClass::Outside
    );
}
