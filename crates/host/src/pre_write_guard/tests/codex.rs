//! The `.codex` rule (clauseo-acp-private#184): Codex patches into the matter's `.codex` folder,
//! through the `apply_patch` tool or a `Bash` command.

use std::path::Path;

use serde_json::json;

use super::{Fixture, deny, guard_json};
#[cfg(unix)]
use crate::write;

/// The deny line for a Codex patch into the matter's `.codex` folder.
fn codex_refused() -> String {
    "Clauseo doesn't let the agent change the .codex folder in a matter: files there change \
     what the agent loads and runs.\n"
        .to_owned()
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

/// Codex's `exec_command` applies `apply_patch <<'EOF' …` sent through `Bash` as a patch
/// (review probe P2): a `.codex` part anywhere refuses it, since the command's working folder is
/// unknown, and a held file it updates is refused as a held file.
#[test]
fn a_patch_sent_through_bash_is_checked_too() {
    let fx = Fixture::new("bash-patch");
    for command in [
        "apply_patch <<'EOF'\n*** Begin Patch\n*** Add File: .codex/config.toml\n+x\n*** End Patch\nEOF"
            .to_owned(),
        format!(
            "apply_patch <<'EOF'\n*** Begin Patch\n*** Add File: {}\n+x\n*** End Patch\nEOF",
            fx.path("m/sub/.CODEX/hooks.json")
        ),
        "cd sub && apply_patch <<'EOF'\n*** Begin Patch\n*** Update File: x\n*** Move to: \
         ../.codex/config.toml\n*** End Patch\nEOF"
            .to_owned(),
    ] {
        assert_eq!(
            guard_json(&fx.hook("Bash", &json!({"command": command}))),
            codex_refused(),
            "{command}"
        );
    }
    let held = "apply_patch <<'EOF'\n*** Begin Patch\n*** Update File: Contract.docx\n@@\n-a\n+b\n\
                *** End Patch\nEOF";
    assert_eq!(
        guard_json(&fx.hook("Bash", &json!({"command": held}))),
        deny("Contract.docx")
    );
    let elsewhere =
        "apply_patch <<'EOF'\n*** Begin Patch\n*** Add File: notes.md\n+x\n*** End Patch\nEOF";
    assert_eq!(
        guard_json(&fx.hook("Bash", &json!({"command": elsewhere}))),
        ""
    );
}

/// Review probe P3: an absolute path through a symlinked alias of the matter, to a `.codex`
/// folder that does not exist yet, is refused.
#[cfg(unix)]
#[test]
fn a_codex_patch_through_an_alias_of_the_matter_is_refused_before_codex_exists() {
    let fx = Fixture::new("alias-codex");
    write::symlink(&fx.matter(), &fx.root.join("alias")).unwrap();
    let body = format!("*** Add File: {}", fx.path("alias/.codex/config.toml"));
    assert_eq!(
        guard_json(&fx.hook("apply_patch", &json!({"command": patch(&body)}))),
        codex_refused()
    );
}
