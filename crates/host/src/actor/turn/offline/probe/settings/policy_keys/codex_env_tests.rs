use base64::Engine as _;

use super::{codex_refusal, plist_sets, toml_sets};

/// A fresh folder for `test` holding `name` with `text`, and the file's path.
fn file(test: &str, name: &str, text: &[u8]) -> std::path::PathBuf {
    let path = super::super::super::tests::folder(test).join(name);
    crate::write::write_file(&path, text).unwrap();
    path
}

#[test]
fn each_shell_environment_policy_key_refuses_at_the_top_or_in_a_profile() {
    for text in [
        "[shell_environment_policy]\nset = { PATH = \"/usr/bin\" }\n",
        "[shell_environment_policy]\ninherit = \"all\"\n",
        "[shell_environment_policy]\nexclude = [\"PATH\"]\n",
        "[shell_environment_policy]\nignore_default_excludes = false\n",
        "shell_environment_policy = {}\n",
        "[profiles.firm.shell_environment_policy]\ninclude_only = [\"HOME\"]\n",
        "profiles = 1\n",
        "not toml = = =\n",
    ] {
        assert!(toml_sets(text.as_bytes()), "{text}");
    }
    for text in [
        "",
        "model = \"o4\"\n",
        "[profiles.firm]\nmodel = \"o4\"\n",
        "\u{feff}approval_policy = \"never\"\n",
    ] {
        assert!(!toml_sets(text.as_bytes()), "{text}");
    }
    assert!(toml_sets(b"\xff\xfe"));
}

#[test]
fn a_managed_file_that_sets_it_or_cannot_be_read_refuses_the_launch() {
    let sets = file(
        "codex-sets",
        "managed_config.toml",
        b"[shell_environment_policy]\ninherit = \"all\"\n",
    );
    let plain = file("codex-plain", "config.toml", b"model = \"o4\"\n");
    let absent = plain.with_file_name("absent.toml");
    assert!(codex_refusal(&[plain.clone(), absent]).is_ok());
    let error = codex_refusal(&[plain.clone(), sets]).unwrap_err();
    assert_eq!(
        error.code(),
        clauseo_protocol::ErrorCode::AgentFirmPolicyWeakens
    );
    // A folder where the file should be cannot be read as one.
    let folder = plain.parent().unwrap().to_path_buf();
    assert!(codex_refusal(&[folder]).is_err());
}

/// A managed-preferences property list with `config_toml_base64` holding `layer`.
fn plist_with(layer: &str) -> Vec<u8> {
    let encoded = base64::engine::general_purpose::STANDARD.encode(layer);
    let mut items = plist::Dictionary::new();
    items.insert(
        "config_toml_base64".to_owned(),
        plist::Value::String(encoded),
    );
    let mut bytes = Vec::new();
    plist::Value::Dictionary(items)
        .to_writer_xml(&mut bytes)
        .unwrap();
    bytes
}

#[test]
fn a_managed_preferences_layer_that_sets_it_refuses() {
    assert!(plist_sets(&plist_with(
        "[shell_environment_policy]\nset = { PATH = \"/usr/bin\" }\n"
    )));
    assert!(!plist_sets(&plist_with("model = \"o4\"\n")));
    assert!(!plist_sets(b""));
    assert!(plist_sets(b"not a plist"));
    let mut items = plist::Dictionary::new();
    items.insert("other".to_owned(), plist::Value::Boolean(true));
    let mut bytes = Vec::new();
    plist::Value::Dictionary(items)
        .to_writer_xml(&mut bytes)
        .unwrap();
    assert!(!plist_sets(&bytes));
}
