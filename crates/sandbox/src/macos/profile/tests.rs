use std::path::{Component, Path, PathBuf};

use super::*;
use crate::test_support::TempDir;
use crate::{LinkFinding, LinkKind, PolicyBuilder};

const TAG: &str = "clauseo-4242-7";

/// A matter, its state folder, a home and a data dir in one temp folder.
struct World {
    dir: TempDir,
}

impl World {
    fn new(test: &str) -> World {
        let dir = TempDir::new(test);
        for name in [
            "matter",
            "data/agent-state/m1/codex",
            "data/agent-state/m1/notes",
            "data/agents/codex-acp",
            "home",
            "elsewhere",
        ] {
            dir.dir(name);
        }
        World { dir }
    }

    fn at(&self, rel: &str) -> PathBuf {
        self.dir.path().join(rel)
    }

    fn builder(&self) -> PolicyBuilder {
        Policy::builder(self.at("matter"), self.at("data/agent-state/m1/codex"))
            .home(self.at("home"))
            .hidden(self.at("data"))
    }

    /// The profile text, then each parameter with its path relative to the temp folder and
    /// written with `/`, so the snapshot is the same on every OS.
    fn render(&self, policy: &Policy) -> String {
        let profile = SeatbeltProfile::new(policy, TAG).unwrap();
        let root = dunce::canonicalize(self.dir.path()).unwrap();
        let mut out = profile.text().to_owned();
        out.push_str("\n;; parameters\n");
        for (name, path) in profile.params() {
            out.push_str(name);
            out.push_str(" = ");
            out.push_str(&relative(&root, path));
            out.push('\n');
        }
        out
    }
}

fn relative(root: &Path, path: &Path) -> String {
    let Ok(rest) = path.strip_prefix(root) else {
        return format!("<outside the temp folder: {}>", path.display());
    };
    let parts: Vec<String> = rest
        .components()
        .filter_map(|c| match c {
            Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
            Component::Prefix(_)
            | Component::RootDir
            | Component::CurDir
            | Component::ParentDir => None,
        })
        .collect();
    format!("<root>/{}", parts.join("/"))
}

#[test]
fn matter_only() {
    let w = World::new("profile-matter-only");
    let policy = w.builder().build().unwrap();
    insta::assert_snapshot!(w.render(&policy));
}

#[test]
fn with_a_credential_link() {
    let w = World::new("profile-credential-link");
    let policy = w
        .builder()
        .credential_link(w.at("data/agent-state/m1/codex/auth.json"))
        .notes(w.at("data/agent-state/m1/notes"))
        .proxy_port(41_234)
        .build()
        .unwrap();
    insta::assert_snapshot!(w.render(&policy));
}

#[test]
fn with_an_allowed_folder() {
    let w = World::new("profile-allowed-folder");
    w.dir.dir("elsewhere/Precedents");
    let policy = w
        .builder()
        .allowed_folder(w.at("elsewhere/Precedents"))
        .read(w.at("elsewhere"))
        .loopback(Loopback::Ports(vec![9222, 9223]))
        .build()
        .unwrap();
    insta::assert_snapshot!(w.render(&policy));
}

#[test]
fn with_read_only_paths_in_the_matter() {
    let w = World::new("profile-read-only");
    w.dir.file("matter/.git/config", "[core]\n");
    w.dir.file("matter/sub/.git/HEAD", "ref: refs/heads/main\n");
    let big = w.dir.file("matter/big.pdf", "large");
    w.dir.file("elsewhere/secret.txt", "secret");
    let policy = w
        .builder()
        .over_cap([big])
        .read_only_in_state(w.at("data/agent-state/m1/codex/config.toml"))
        .links([LinkFinding {
            path: dunce::canonicalize(w.at("matter"))
                .unwrap()
                .join("to-secret"),
            kind: LinkKind::Symlink,
            target: Some(dunce::canonicalize(w.at("elsewhere/secret.txt")).unwrap()),
        }])
        .build()
        .unwrap();
    insta::assert_snapshot!(w.render(&policy));
}

#[test]
fn with_the_claude_store() {
    let w = World::new("profile-claude-store");
    w.dir.dir("home/.claude");
    let policy = w
        .builder()
        .claude_store(w.at("home/.claude"))
        .keychain(true)
        .loopback(Loopback::Any)
        .build()
        .unwrap();
    insta::assert_snapshot!(w.render(&policy));
}

#[test]
fn with_a_runtime_path_in_the_data_dir() {
    let w = World::new("profile-runtime-in-data");
    let policy = w
        .builder()
        .runtime(w.at("data/agents/codex-acp"))
        .hidden(w.at("elsewhere"))
        .build()
        .unwrap();
    insta::assert_snapshot!(w.render(&policy));
}

// Windows refuses `"` and newlines in a name.
#[cfg(unix)]
#[test]
fn paths_reach_the_text_only_as_parameters() {
    let w = World::new("profile-quotes");
    let odd = w.dir.dir("elsewhere/a\"b) (allow default)\n(c");
    let policy = w.builder().read(&odd).allowed_folder(&odd).build().unwrap();
    let profile = SeatbeltProfile::new(&policy, TAG).unwrap();
    assert!(
        !profile.text().contains("allow default"),
        "{}",
        profile.text()
    );
    let root = dunce::canonicalize(w.dir.path()).unwrap();
    let root = root.to_string_lossy();
    assert!(!profile.text().contains(&*root), "a path is in the text");
    assert!(
        profile
            .params()
            .iter()
            .any(|(_, path)| path.ends_with(odd.file_name().unwrap()))
    );
}

#[test]
fn a_link_target_that_holds_the_matter_gets_no_rule() {
    let w = World::new("profile-link-up");
    let policy = w
        .builder()
        .links([LinkFinding {
            path: w.at("matter/up"),
            kind: LinkKind::Symlink,
            target: Some(dunce::canonicalize(w.dir.path()).unwrap()),
        }])
        .build()
        .unwrap();
    let profile = SeatbeltProfile::new(&policy, TAG).unwrap();
    assert!(profile.params().iter().any(|(name, _)| name == "LINK_0"));
    assert!(
        !profile
            .params()
            .iter()
            .any(|(name, _)| name.starts_with("LINK_TARGET"))
    );
}

#[test]
fn every_deny_rule_carries_the_tag() {
    let w = World::new("profile-tags");
    w.dir.file("matter/.git/config", "[core]\n");
    let policy = w.builder().hidden(w.at("elsewhere")).build().unwrap();
    let text = SeatbeltProfile::new(&policy, TAG)
        .unwrap()
        .text()
        .to_owned();
    let message = format!("(with message \"{TAG}\")");
    let mut rules: Vec<String> = Vec::new();
    for line in text.lines() {
        if line.starts_with('(') || line.starts_with(';') || line.is_empty() {
            rules.push(line.to_owned());
        } else if let Some(rule) = rules.last_mut() {
            rule.push_str(line);
        }
    }
    let denies: Vec<&String> = rules.iter().filter(|r| r.starts_with("(deny")).collect();
    assert!(denies.len() >= 4, "{text}");
    for rule in denies {
        assert!(rule.contains(&message), "untagged deny rule: {rule}");
    }
}

#[test]
fn tags_are_checked() {
    let w = World::new("profile-bad-tags");
    let policy = w.builder().build().unwrap();
    for tag in [
        "",
        "clauseo-",
        "other-1",
        "clauseo-a\"b",
        "clauseo-a b",
        "clauseo-a\n",
    ] {
        assert!(
            matches!(SeatbeltProfile::new(&policy, tag), Err(Error::Policy(_))),
            "{tag:?}"
        );
    }
    let long = format!("clauseo-{}", "a".repeat(60));
    assert!(SeatbeltProfile::new(&policy, &long).is_err());
}

#[test]
fn args_are_the_text_then_each_parameter() {
    let w = World::new("profile-args");
    let policy = w.builder().build().unwrap();
    let profile = SeatbeltProfile::new(&policy, TAG).unwrap();
    let args = profile.args();
    assert_eq!(args[0], "-p");
    assert_eq!(args[1], profile.text());
    assert_eq!(args.len(), 2 + 2 * profile.params().len());
    let (name, path) = &profile.params()[0];
    assert_eq!(args[2], "-D");
    let mut pair = std::ffi::OsString::from(name);
    pair.push("=");
    pair.push(path);
    assert_eq!(args[3], pair);
}
