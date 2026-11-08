//! The template's two Authenticode blocks (S20a, decision 0035): `signed-plugins` adds a folder of
//! signed copies of the NSIS plug-ins, `sign-uninstaller` runs a command on the uninstaller through
//! `!uninstfinalize`. Both are compile-time `!if`s on environment variables that only release.yml's
//! sign-windows sets, when Authenticode signing is on, so a build without them is as before.

use super::sources::read;
use super::template::{TEMPLATE, block_code, split_blocks};

/// The `StrFunc` declarations the template makes before the plug-in folders (they define string
/// functions and call no plug-in).
const STRFUNC: [&str; 2] = ["${StrCase}", "${StrLoc}"];

/// The NSIS lines between `from` and `to` in `lines` (both included), comments and blank lines
/// left out, trimmed.
fn code_between<'a>(lines: &[&'a str], from: usize, to: usize) -> Vec<&'a str> {
    lines[from..=to]
        .iter()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty() && !line.starts_with(';') && !line.starts_with('#'))
        .collect()
}

#[test]
fn the_signing_blocks_do_nothing_unless_their_switch_is_set() {
    let (blocks, _) = split_blocks(&read(TEMPLATE));
    // Each is one compile-time `!if` on a switch that an unset variable can't equal: makensis
    // leaves an undefined $%NAME% as written.
    assert_eq!(
        block_code(&blocks, "signed-plugins"),
        [
            "!if \"$%CLAUSEO_SIGNED_PLUGINS%\" == \"1\"",
            "!addplugindir \"$%CLAUSEO_SIGNED_PLUGINS_DIR%\"",
            "!endif",
        ]
    );
    assert_eq!(
        block_code(&blocks, "sign-uninstaller"),
        [
            "!if \"$%CLAUSEO_SIGN_UNINSTALLER%\" == \"1\"",
            "!uninstfinalize '$%CLAUSEO_UNINSTALLER_COMMAND% \"%1\"' = 0",
            "!endif",
        ]
    );
    // The signed plug-ins' folder is the last one added (makensis takes a plug-in from the last
    // folder that has it), and no plug-in is called before it: nothing above the block but
    // directives, includes and the version keys.
    let text = read(TEMPLATE);
    let lines: Vec<&str> = text.split('\n').collect();
    let block = lines
        .iter()
        .position(|line| line.trim() == "; <<< clauseo: signed-plugins")
        .unwrap();
    let after: Vec<&str> = lines[block..]
        .iter()
        .copied()
        .filter(|line| line.trim_start().starts_with("!addplugindir"))
        .collect();
    assert_eq!(
        after,
        Vec::<&str>::new(),
        "a plug-in folder added after the signed one"
    );
    for line in code_between(&lines, 0, block) {
        assert!(
            !line.contains("::"),
            "a plug-in call before the signed plug-ins: {line}"
        );
        let word = line.split_whitespace().next().unwrap_or_default();
        // StrFunc's declarations (${StrCase}, ${StrLoc}) define string functions, no plug-in.
        assert!(
            word.starts_with('!')
                || STRFUNC.contains(&word)
                || word.starts_with('{')
                || [
                    "Unicode",
                    "ManifestDPIAware",
                    "ManifestDPIAwareness",
                    "SetCompress",
                    "SetCompressor",
                    "Var",
                    "Name",
                    "BrandingText",
                    "OutFile",
                    "InstallDir",
                    "VIProductVersion",
                    "VIAddVersionKey",
                ]
                .contains(&word),
            "{line}"
        );
    }
    // The uninstaller's command follows Tauri's own uninstaller signing, which stays unused
    // (release.yml bundles with --no-sign).
    let ours = lines
        .iter()
        .position(|line| line.trim() == "; >>> clauseo: sign-uninstaller")
        .unwrap();
    assert_eq!(
        code_between(&lines, ours - 4, ours - 1),
        [
            "!if \"${UNINSTALLERSIGNCOMMAND}\" != \"\"",
            "!uninstfinalize '${UNINSTALLERSIGNCOMMAND}'",
            "!endif",
        ]
    );
}
