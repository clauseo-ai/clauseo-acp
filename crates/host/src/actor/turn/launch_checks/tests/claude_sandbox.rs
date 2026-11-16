//! Claude's sandbox against the real run folder's paths, and the other matters' rule a Codex
//! launch takes. Split from `tests.rs` for its size.

use std::path::{Path, PathBuf};

use super::folders;

/// Claude's sandbox opens only the named folders of the real run folder: never the run folder
/// whole, the relay's token, agent-browser's folder, its config or the browser's profile (the
/// visited sites' cookies), but the browser's downloads.
#[test]
#[cfg_attr(windows, ignore = "Claude's sandbox is macOS and Linux only")]
fn claudes_sandbox_opens_no_secret_of_the_real_run_folder() {
    use crate::agent::loads::sandbox::{ClaudeSandbox, SandboxOs, SandboxPaths};
    let data = PathBuf::from("/Users/l/Library/Application Support/Clauseo");
    let run = folders(&data, 1);
    let tmp = crate::agent::temp::temp_dir(&run.root);
    let short = PathBuf::from("/tmp/clsb-abcd1234");
    let matter = PathBuf::from("/Users/l/Clients/A");
    let paths = SandboxPaths {
        matter: &matter,
        work: &run.work,
        tmp: &tmp,
        claude_tmp: Some(&short),
        downloads: &run.downloads,
        skill_packs: &run.skill_packs,
        notes: &run.notes,
        skills: &[],
        path: &[],
        env_file: Some(&run.root.join(crate::agent::shell_path::ENV_FILE)),
        ca_file: Some(&data.join("tls/roots.pem")),
        claude_home: &data.join("agents/claude-acp/home"),
        data: &run.data_deny.spellings,
        other_matters: &[],
        matter_shut: false,
        matter_inside: &[],
    };
    let sandbox = ClaudeSandbox::for_os(SandboxOs::Mac, &paths).unwrap();
    let text = |path: &Path| path.to_str().unwrap().to_owned();
    for secret in [
        &run.root,
        &run.relay_token,
        &run.agent_browser,
        &run.browser_config,
        &run.browser_profile,
        &run.browser,
    ] {
        let secret = text(secret);
        for open in sandbox.allow_read.iter().chain(&sandbox.allow_write) {
            assert!(!secret.starts_with(open.as_str()), "{open} opens {secret}");
        }
    }
    assert!(sandbox.allow_read.contains(&text(&run.downloads)));
    assert!(text(&run.downloads).ends_with("agent-browser/downloads"));
    assert!(sandbox.deny_read.contains(&text(&data)));
}

/// Codex's instructions name no matter on any OS (no other client's matter goes to the model's
/// provider, and a resumed thread never reads them again): its rule is fixed, the fenced one off
/// Windows; the note names the invited matters alone, by name.
#[test]
fn codex_s_rule_names_no_matter_and_its_note_only_the_invited() {
    use clauseo_profile::instructions::OTHER_MATTERS;

    use crate::actor::turn::outside::fence::{codex_reads_note, codex_rule};
    use crate::agent::matter_fence::{MatterFence, Other};

    let data = PathBuf::from(crate::testing::abs("/Users/l/AppData/Local/Clauseo"));
    let mut run = folders(&data, 1);
    let id = |n: u128| {
        let bits = (0x0190_0000_0000_u128 << 80) | (0x7 << 76) | (0b10 << 62) | n;
        clauseo_protocol::MatterId::new(uuid::Uuid::from_u128(bits)).unwrap()
    };
    let other = |n, name: &str, invited| {
        let folder = PathBuf::from(crate::testing::abs(&format!("/Clients/{name}")));
        Other::new(id(n), name.to_owned(), folder, invited)
    };
    run.data_deny.matters =
        MatterFence::asked(vec![other(1, "Smith", false), other(2, "Jones", true)]);
    assert_eq!(codex_rule(false), OTHER_MATTERS.trim());
    for rule in [codex_rule(false), codex_rule(true)] {
        assert!(!rule.contains("Smith") && !rule.contains("Jones"), "{rule}");
    }
    assert!(codex_rule(true).contains("`.clauseo`"));
    assert!(codex_rule(true).contains("\"Let this conversation read\""));
    let note = codex_reads_note(&run);
    assert!(note.contains("\"Jones\""), "{note}");
    assert!(!note.contains("Smith"), "{note}");
    run.data_deny.matters = MatterFence::asked(vec![other(1, "Smith", false)]);
    assert!(codex_reads_note(&run).ends_with("may read no other matter."));
}
