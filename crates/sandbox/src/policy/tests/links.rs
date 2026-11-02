//! Link findings in the policy: canonical, inside the matter, and within the rule budget
//! (review round 1, items 2, 3 and 5).

use super::*;
use crate::{LinkKind, MatterScan};

fn hard(path: PathBuf) -> LinkFinding {
    LinkFinding {
        path,
        kind: LinkKind::HardLink,
        target: None,
    }
}

#[test]
fn findings_are_made_canonical_and_must_be_inside_the_matter() {
    let w = World::new("findings-canon");
    let target = w.dir.file("other/secret.txt", "s");
    // The temp folder's path is not canonical on macOS (/var is /private/var).
    let policy = w
        .builder()
        .scan(MatterScan::default())
        .links([LinkFinding {
            path: w.at("matter/hl"),
            kind: LinkKind::Symlink,
            target: Some(w.at("other/secret.txt")),
        }])
        .build()
        .unwrap();
    assert_eq!(
        policy.links(),
        [LinkFinding {
            path: canon(&w.at("matter")).join("hl"),
            kind: LinkKind::Symlink,
            target: Some(canon(&target)),
        }]
    );
    refused(
        w.builder()
            .scan(MatterScan::default())
            .links([hard(w.at("other/secret.txt"))])
            .build(),
        "a link finding must be inside the matter",
        &w.at("other/secret.txt"),
    );
    refused(
        w.builder()
            .scan(MatterScan::default())
            .links([hard(w.at("matter"))])
            .build(),
        "a link finding must be inside the matter",
        &w.at("matter"),
    );
}

#[test]
fn findings_past_the_budget_give_way_to_their_nearest_folders() {
    let w = World::new("findings-budget");
    let n = LINK_RULES_MAX + 44;
    let mut findings = Vec::new();
    for i in 0..n {
        w.dir.dir(&format!("matter/a/f{i}"));
        findings.push(hard(w.at(&format!("matter/a/f{i}/doc.pdf"))));
    }
    // A few under the budget elsewhere keep their own rules.
    w.dir.dir("matter/b");
    findings.push(hard(w.at("matter/b/one.pdf")));
    let policy = w
        .builder()
        .scan(MatterScan::default())
        .links(findings)
        .build()
        .unwrap();
    let matter = canon(&w.at("matter"));
    // One level up still has n folders; two levels up is `a`. `b/one.pdf` sits at the
    // shallower level and moves up last, so it stays a rule of its own.
    assert_eq!(policy.link_folders(), [matter.join("a")]);
    let own: Vec<&Path> = policy
        .links_outside_link_folders()
        .map(|f| f.path.as_path())
        .collect();
    assert_eq!(own, [matter.join("b/one.pdf")]);
    assert_eq!(policy.links().len(), n + 1);
    assert_eq!(
        policy.classify(&matter.join("a/f3/doc.pdf"), Access::Read),
        PathClass::LinkTarget
    );
}

#[test]
fn findings_that_cannot_fit_are_refused_with_the_count() {
    let w = World::new("findings-refused");
    let n = LINK_RULES_MAX + 1;
    let findings = (0..n).map(|i| hard(w.at(&format!("matter/doc{i}.pdf"))));
    let result = w
        .builder()
        .scan(MatterScan::default())
        .links(findings)
        .build();
    refused(
        result,
        &format!("the matter has {n} links"),
        &w.at("matter"),
    );
}

#[cfg(unix)]
#[test]
fn a_folder_holding_a_git_is_never_a_link_folder() {
    let w = World::new("findings-git");
    w.dir.dir("matter/repo/.git");
    w.dir.dir("matter/repo/docs");
    let outside = w.dir.file("other/o.txt", "o");
    for i in 0..=LINK_RULES_MAX {
        w.dir
            .hard_link(&format!("matter/repo/doc{i}.txt"), &outside);
    }
    refused(
        w.builder().build(),
        &format!("the matter has {} links", LINK_RULES_MAX + 1),
        &w.at("matter"),
    );
    // In a sub-folder of the working tree they fall back to it.
    for i in 0..=LINK_RULES_MAX {
        let rel = format!("matter/repo/doc{i}.txt");
        crate::test_support::remove(&w.at(&rel));
        w.dir
            .hard_link(&format!("matter/repo/docs/doc{i}.txt"), &outside);
    }
    let policy = w.builder().build().unwrap();
    assert_eq!(
        policy.link_folders(),
        [canon(&w.at("matter")).join("repo/docs")]
    );
}

#[cfg(unix)]
#[test]
fn a_folder_above_a_working_tree_is_never_a_link_folder() {
    // The round 2 reviewer's case: `projects/` would hide `projects/repo` and its files.
    let w = World::new("findings-above-git");
    w.dir
        .file("matter/projects/repo/.git/HEAD", "ref: refs/heads/main\n");
    w.dir.file("matter/projects/repo/brief.txt", "brief\n");
    for i in 0..300 {
        let outside = w.dir.file(&format!("other/o{i}.txt"), "o");
        w.dir.dir(&format!("matter/projects/x{i}"));
        w.dir
            .hard_link(&format!("matter/projects/x{i}/f"), &outside);
    }
    refused(
        w.builder().build(),
        "the matter has 300 links",
        &w.at("matter"),
    );
}

#[cfg(unix)]
#[test]
fn symlink_targets_nothing_allows_take_no_rules() {
    let w = World::new("findings-targets");
    w.dir.dir("matter/links");
    for i in 0..300 {
        let outside = w.dir.file(&format!("other/o{i}.txt"), "o");
        w.dir.symlink(&format!("matter/links/l{i}"), &outside);
    }
    // One target inside a read path keeps its rule: the read path's allow would let it through.
    let skill = w.dir.file("skills/s.md", "s");
    w.dir.symlink("matter/links/skill", &skill);
    let policy = w.builder().read(w.at("skills")).build().unwrap();
    assert_eq!(policy.links().len(), 301);
    assert_eq!(policy.link_targets(), [canon(&skill)]);
    assert_eq!(
        policy.link_folders(),
        [canon(&w.at("matter")).join("links")]
    );
}

#[cfg(unix)]
#[test]
fn a_hard_link_from_git_to_a_writable_name_is_a_finding() {
    let w = World::new("findings-mixed");
    let config = w.dir.file("matter/.git/config", "[core]\n");
    w.dir.hard_link("matter/cfg", &config);
    let plain = w.dir.file("matter/a.txt", "a");
    w.dir.hard_link("matter/b.txt", &plain);
    let outside = w.dir.file("other/secret.txt", "s");
    w.dir.hard_link("matter/.git/objects-pack", &outside);
    let policy = w.builder().build().unwrap();
    let matter = canon(&w.at("matter"));
    let mut found: Vec<&Path> = policy.links().iter().map(|f| f.path.as_path()).collect();
    found.sort();
    assert_eq!(
        found,
        [matter.join(".git/objects-pack"), matter.join("cfg")]
    );
}
