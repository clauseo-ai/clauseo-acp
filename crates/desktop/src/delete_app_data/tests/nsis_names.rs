//! The names the installer scripts may use: every `$NAME` in hooks.nsh, English.nsh and Hindi.nsh
//! is one NSIS knows, a register, or one of the template's variables that the hooks use. NSIS keeps
//! an unknown name as literal text (the installer showed `$COMMONAPPDATA` unexpanded), so this test
//! is what fails a misspelt name before a build.

use super::sources::{hook_code, line_with, read};

/// The `$NAME`s in `line`: the letters, digits and underscores after each `$`. `$(…)`, `${…}`, `$$`
/// and `$\r`-style escapes name nothing and give no entry.
fn variable_names(line: &str) -> Vec<&str> {
    line.match_indices('$')
        .filter_map(|(at, _)| {
            let rest = &line[at + 1..];
            let len = rest
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .unwrap_or(rest.len());
            (len > 0).then(|| &rest[..len])
        })
        .collect()
}

#[test]
fn the_installer_scripts_use_only_known_nsis_variables() {
    // NSIS's built-in constants: the list in the NSIS 3 docs (Docs/src/var.but, "Constants"), with
    // the $USER../$COMMON.. aliases the docs name, and the three more that the compiler knows
    // (Source/build.cpp, the shell constants: USERAPPDATA, USERLOCALAPPDATA, COMMONLOCALAPPDATA).
    // An unknown name is no error to NSIS: it stays as the literal text, which is how
    // $COMMONAPPDATA reached the installer's details list. hooks.nsh's `!pragma warning error 6000`
    // makes that warning fatal at compile time.
    const NSIS_CONSTANTS: &[&str] = &[
        "CMDLINE",
        "INSTDIR",
        "OUTDIR",
        "LANGUAGE",
        "EXEDIR",
        "EXEFILE",
        "EXEPATH",
        "HWNDPARENT",
        "PLUGINSDIR",
        "TEMP",
        "PROGRAMFILES",
        "PROGRAMFILES32",
        "PROGRAMFILES64",
        "COMMONFILES",
        "COMMONFILES32",
        "COMMONFILES64",
        "DESKTOP",
        "WINDIR",
        "SYSDIR",
        "STARTMENU",
        "SMPROGRAMS",
        "SMSTARTUP",
        "QUICKLAUNCH",
        "DOCUMENTS",
        "SENDTO",
        "RECENT",
        "FAVORITES",
        "MUSIC",
        "PICTURES",
        "VIDEOS",
        "NETHOOD",
        "FONTS",
        "TEMPLATES",
        "APPDATA",
        "LOCALAPPDATA",
        "PRINTHOOD",
        "INTERNET_CACHE",
        "COOKIES",
        "HISTORY",
        "PROFILE",
        "ADMINTOOLS",
        "RESOURCES",
        "RESOURCES_LOCALIZED",
        "CDBURN_AREA",
        "USERTEMPLATES",
        "USERSTARTMENU",
        "USERSMPROGRAMS",
        "USERDESKTOP",
        "USERAPPDATA",
        "USERLOCALAPPDATA",
        "COMMONTEMPLATES",
        "COMMONSTARTMENU",
        "COMMONSMPROGRAMS",
        "COMMONDESKTOP",
        "COMMONPROGRAMDATA",
        "COMMONLOCALAPPDATA",
    ];
    // The Tauri template's own variables that the hooks use (tauri-cli 2.12.2, installer.nsi).
    const TEMPLATE_VARS: &[&str] = &[
        // `Var UpdateMode` (line 75): 1 when the uninstall is an update's (/UPDATE).
        "UpdateMode",
        // `Var DeleteAppDataCheckboxState` (line 427): the "delete app data" box, which the
        // uninstall hook reads.
        "DeleteAppDataCheckboxState",
    ];
    // hooks.nsh's own `Var`s: 1 when a previous folder was refused, and 1 when the per-machine
    // installer ignored a vendor key that differed from InstallLocation.
    const HOOK_VARS: &[&str] = &["ClauseoPreviousIgnored", "ClauseoPreviousKeyMismatch"];
    // Registers $0-$9 and $R0-$R9 need no declaration.
    let digit = |s: &str| s.len() == 1 && s.bytes().all(|b| b.is_ascii_digit());
    let register = |name: &str| digit(name) || name.strip_prefix('R').is_some_and(digit);
    let mut checked = 0;
    for file in [
        "windows/hooks.nsh",
        "windows/nsis/English.nsh",
        "windows/nsis/Hindi.nsh",
    ] {
        for (number, line) in read(file).lines().enumerate() {
            for name in variable_names(line) {
                checked += 1;
                let known = register(name)
                    || NSIS_CONSTANTS.contains(&name)
                    || TEMPLATE_VARS.contains(&name)
                    || HOOK_VARS.contains(&name);
                assert!(
                    known,
                    "{file}:{}: ${name} is not an NSIS variable: {line}",
                    number + 1
                );
            }
        }
    }
    assert!(checked > 40, "only {checked} variables read");
    let code = hook_code();
    line_with(&code, &["!pragma warning error 6000"]);
}
