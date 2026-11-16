//! A data folder's name alone (`forms::alone_name`): Clauseo's own name, a configured name
//! holding `clauseo` and the 8.3 short form of either are written as the name alone; a generic
//! name, its short form, and a name the own matter's folder holds keep the path forms.

use std::path::PathBuf;

use super::forms::{alone_name, forms, is_short_of};
use super::tests::{denied_by, env};
use super::windows_shell_deny_rules;
use crate::agent::data_fence::LaunchDeny;

const LONG: &str = r"C:\Users\Asha\AppData\Local\ai.clauseo.desktop";
const SHORT: &str = r"C:\Users\ASHA~1\AppData\Local\AICLAU~1.DES";

fn launch(spellings: &[&str]) -> LaunchDeny {
    LaunchDeny::fixed(spellings.iter().map(PathBuf::from).collect())
}

/// The leads of `deny`'s forms, and whether each is a name alone.
fn leads(deny: &LaunchDeny) -> Vec<(String, bool)> {
    forms(deny, &env, true)
        .into_iter()
        .map(|form| (form.lead, form.alone))
        .collect()
}

#[test]
fn the_short_data_spelling_is_its_name_alone() {
    let deny = launch(&[LONG, SHORT]);
    assert_eq!(
        leads(&deny),
        vec![
            ("ai.clauseo.desktop".to_owned(), true),
            ("AICLAU~1.DES".to_owned(), true)
        ]
    );
    let rules = windows_shell_deny_rules(&deny, &env);
    for command in [
        r"type C:\Users\ASHA~1\AppData\Local\AICLAU~1.DES\settings.json",
        "cat C:/Users/ASHA~1/AppData/Local/AICLAU~1.DES/settings.json",
        // A mixed spelling, long parents and the short last name: no path form had it.
        r"type %LOCALAPPDATA%\AICLAU~1.DES\settings.json",
        r"type C:\Users\Asha\AppData\Local\AICLAU~1.DES\agents\signin\auth.json",
        r#"dir "C:\Users\ASHA~1\AppData\Local\AICLAU~1.DES""#,
    ] {
        assert!(denied_by(&rules, "Bash", command), "Bash: {command}");
        assert!(
            denied_by(&rules, "PowerShell", command),
            "PowerShell: {command}"
        );
    }
    // PowerShell in any case; Bash as written (the residual, as for the long name).
    let lower = r"get-content c:\users\asha~1\appdata\local\aiclau~1.des\settings.json";
    assert!(denied_by(&rules, "PowerShell", lower));
}

#[test]
fn a_configured_name_holding_clauseo_is_its_name_alone() {
    let deny = launch(&[r"D:\Firm\Clauseo Data", r"D:\Firm\CLAUSE~1"]);
    assert_eq!(
        leads(&deny),
        vec![
            ("Clauseo Data".to_owned(), true),
            ("CLAUSE~1".to_owned(), true)
        ]
    );
    let rules = windows_shell_deny_rules(&deny, &env);
    for command in [
        r"type D:\Firm\Clauseo Data\settings.json",
        r#"type "\\localhost\D$\Firm\Clauseo Data\settings.json""#,
        r"type D:\Firm\CLAUSE~1\state.sqlite",
    ] {
        assert!(denied_by(&rules, "PowerShell", command), "{command}");
    }
}

#[test]
fn a_generic_name_and_its_short_form_keep_the_path_forms() {
    let deny = launch(&[r"D:\FirmData\Records", r"D:\FIRMDA~1\RECORD~1"]);
    let leads = leads(&deny);
    assert!(leads.iter().all(|(_, alone)| !alone), "{leads:?}");
    for lead in [
        r"D:\FirmData\Records",
        "D:/FirmData/Records",
        r"D:\\FirmData\\Records",
        "/d/FirmData/Records",
        r"D:\FIRMDA~1\RECORD~1",
    ] {
        assert!(
            leads.iter().any(|(known, _)| known == lead),
            "{lead}: {leads:?}"
        );
    }
    // A command naming another `Records` folder is not denied.
    let rules = windows_shell_deny_rules(&deny, &env);
    assert!(!denied_by(
        &rules,
        "PowerShell",
        r"type C:\Work\Records\x.txt"
    ));
    assert!(denied_by(
        &rules,
        "PowerShell",
        r"type D:\FirmData\Records\settings.json"
    ));
}

#[test]
fn a_name_the_own_matter_holds_keeps_the_path_forms() {
    let mut deny = launch(&[r"D:\Firm\Clauseo Data"]);
    deny.matters.own = vec![PathBuf::from(r"D:\Work\Clauseo Data Room\Deal")];
    let leads = leads(&deny);
    assert!(leads.iter().all(|(_, alone)| !alone), "{leads:?}");
    let rules = windows_shell_deny_rules(&deny, &env);
    assert!(!denied_by(
        &rules,
        "PowerShell",
        r"type D:\Work\Clauseo Data Room\Deal\x.docx"
    ));
    assert!(denied_by(
        &rules,
        "PowerShell",
        r"type D:\Firm\Clauseo Data\settings.json"
    ));
}

#[test]
fn a_short_name_is_read_as_windows_makes_it() {
    for (short, long) in [
        ("AICLAU~1.DES", "ai.clauseo.desktop"),
        ("aiclau~2.des", "ai.clauseo.desktop"),
        ("AI3F2A~1.DES", "ai.clauseo.desktop"),
        ("CLAUSE~1", "Clauseo Data"),
        ("RECORD~1", "Records"),
    ] {
        assert!(is_short_of(short, long), "{short} of {long}");
    }
    for (short, long) in [
        ("AICLAU~1.TXT", "ai.clauseo.desktop"),
        ("AICLAU.DES", "ai.clauseo.desktop"),
        ("FIRMDA~1", "Records"),
        ("AICLAU~X.DES", "ai.clauseo.desktop"),
        ("ai.clauseo.desktop", "ai.clauseo.desktop"),
    ] {
        assert!(!is_short_of(short, long), "{short} of {long}");
    }
    let names = ["Records".to_owned(), "ai.clauseo.desktop".to_owned()];
    assert!(alone_name("AICLAU~1.DES", &names, &[]));
    assert!(!alone_name("RECORD~1", &names, &[]));
    assert!(!alone_name("Records", &names, &[]));
    assert!(alone_name("My CLAUSEO store", &names, &[]));
}
