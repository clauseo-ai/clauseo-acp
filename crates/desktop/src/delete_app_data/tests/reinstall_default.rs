//! The reinstall page's pick (item 43): with an older or unknown version installed, the first
//! visit checks the second radio, install over it (`dontUninstall`); a later visit keeps the
//! lawyer's own pick; keyboard focus is on whichever radio is checked, so Space or an arrow key
//! never changes the pick from the first paint.

use super::sources::read;
use super::template::{TEMPLATE, block_code, split_blocks};

#[test]
fn an_older_version_defaults_to_install_over_with_focus_on_the_checked_radio() {
    let text = read(TEMPLATE);
    let (blocks, kept) = split_blocks(&text);
    // Only on the first visit ($ReinstallPageCheck empty) and only for an older or unknown
    // version ($R0 1, not 0 the same or -1 newer).
    assert_eq!(
        block_code(&blocks, "reinstall-default"),
        ["${IfThen} \"$R0$ReinstallPageCheck\" == \"1\" ${|} StrCpy $ReinstallPageCheck 2 ${|}"]
    );
    assert_eq!(
        block_code(&blocks, "reinstall-focus"),
        [
            "${If} $ReinstallPageCheck = 2",
            "${NSD_SetFocus} $R3",
            "${Else}",
            "${NSD_SetFocus} $R2",
            "${EndIf}",
        ]
    );
    // The default comes before the page checks a radio from $ReinstallPageCheck, and the focus
    // after, just before the page shows.
    let code: Vec<&str> = kept.iter().map(|line| line.trim()).collect();
    let check = code
        .iter()
        .position(|line| *line == "${If} $ReinstallPageCheck <> 2")
        .unwrap();
    assert_eq!(
        code[check - 1],
        "; selected the last time we were on this page"
    );
    assert_eq!(
        code[check + 1],
        "SendMessage $R2 ${BM_SETCHECK} ${BST_CHECKED} 0"
    );
    assert_eq!(
        code[check + 3],
        "SendMessage $R3 ${BM_SETCHECK} ${BST_CHECKED} 0"
    );
    assert_eq!(code[check + 6], "nsDialogs::Show");
    // The page's own upstream focus line is gone (the block stands in for it).
    assert!(!code.contains(&"${NSD_SetFocus} $R2"));
}
