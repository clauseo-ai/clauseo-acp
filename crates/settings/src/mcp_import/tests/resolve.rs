//! `resolve_command`: each case the host relies on, on both OSes.

use super::*;

/// `resolve_command` over one fake setup.
fn resolve(
    os: Os,
    home: &str,
    pairs: &[(&str, &str)],
    fs: &FakeFs,
    program: &str,
) -> Option<PathBuf> {
    let lookup = vars(pairs);
    let env = Env {
        os,
        home: Path::new(home),
        vars: &lookup,
        fs,
    };
    resolve_command(&env, program)
}

#[test]
fn mac_bare_name_is_searched_on_path_first() {
    let fs = FakeFs::default()
        .program("/custom/bin/tool")
        .program("/opt/homebrew/bin/tool");
    let found = resolve(
        Os::Mac,
        "/Users/ana",
        &[("PATH", "/custom/bin:/usr/bin")],
        &fs,
        "tool",
    );
    assert_eq!(found, Some(PathBuf::from("/custom/bin/tool")));
}

#[test]
fn mac_bare_name_is_found_in_each_well_known_folder() {
    let folders = [
        "/opt/homebrew/bin",
        "/usr/local/bin",
        "/Users/ana/.volta/bin",
        "/Users/ana/.bun/bin",
        "/Users/ana/.local/bin",
        "/Users/ana/.cargo/bin",
    ];
    for folder in folders {
        let path = format!("{folder}/tool");
        let fs = FakeFs::default().program(&path);
        let found = resolve(Os::Mac, "/Users/ana", &[("PATH", "/usr/bin")], &fs, "tool");
        assert_eq!(found, Some(PathBuf::from(&path)), "{folder}");
    }
}

#[test]
fn mac_nvm_uses_the_newest_version_by_number() {
    let fs = FakeFs::default()
        .program("/Users/ana/.nvm/versions/node/v9.11.2/bin/npx")
        .program("/Users/ana/.nvm/versions/node/v10.0.0/bin/npx")
        .program("/Users/ana/.nvm/versions/node/v20.1.0/bin/npx")
        .dir(
            "/Users/ana/.nvm/versions/node",
            &[
                "/Users/ana/.nvm/versions/node/v9.11.2",
                "/Users/ana/.nvm/versions/node/v10.0.0",
                "/Users/ana/.nvm/versions/node/v20.1.0",
            ],
        );
    let found = resolve(Os::Mac, "/Users/ana", &[("PATH", "/usr/bin")], &fs, "npx");
    assert_eq!(
        found,
        Some(PathBuf::from(
            "/Users/ana/.nvm/versions/node/v20.1.0/bin/npx"
        ))
    );
}

#[test]
fn mac_bare_name_not_found_gives_none() {
    let fs = FakeFs::default().program("/usr/bin/other");
    assert_eq!(
        resolve(Os::Mac, "/Users/ana", &[("PATH", "/usr/bin")], &fs, "tool"),
        None
    );
}

#[test]
fn absolute_path_is_returned_only_when_it_is_a_file() {
    let fs = FakeFs::default().program("/opt/tool");
    assert_eq!(
        resolve(Os::Mac, "/Users/ana", &[], &fs, "/opt/tool"),
        Some(PathBuf::from("/opt/tool"))
    );
    assert_eq!(
        resolve(Os::Mac, "/Users/ana", &[], &fs, "/opt/missing"),
        None
    );
}

#[test]
fn relative_paths_with_separators_give_none() {
    let fs = FakeFs::default()
        .program("/Users/ana/bin/tool")
        .program("/usr/bin/tool");
    let pairs = [("PATH", "/Users/ana/bin:/usr/bin")];
    for program in [
        "bin/tool",
        "./tool",
        "../tool",
        "/Users/ana/bin/../bin/tool/",
    ] {
        assert_eq!(
            resolve(Os::Mac, "/Users/ana", &pairs, &fs, program),
            None,
            "{program}"
        );
    }
}

#[test]
fn empty_program_gives_none() {
    let fs = FakeFs::default().program("/usr/bin/tool");
    assert_eq!(
        resolve(Os::Mac, "/Users/ana", &[("PATH", "/usr/bin")], &fs, ""),
        None
    );
}

#[test]
fn windows_bare_name_tries_pathext_in_each_well_known_folder() {
    let pairs = [
        ("APPDATA", "C:\\Users\\ana\\AppData\\Roaming"),
        ("ProgramFiles", "C:\\Program Files"),
        ("LOCALAPPDATA", "C:\\Users\\ana\\AppData\\Local"),
        ("SystemRoot", "C:\\Windows"),
    ];
    let cases = [
        ("npx", "C:\\Users\\ana\\AppData\\Roaming\\npm\\npx.cmd"),
        ("npx", "C:\\Program Files\\nodejs\\npx.cmd"),
        (
            "node",
            "C:\\Users\\ana\\AppData\\Local\\Volta\\bin\\node.exe",
        ),
        ("cmd", "C:\\Windows\\System32\\cmd.exe"),
    ];
    for (program, file) in cases {
        let fs = FakeFs::default().program(file);
        let found = resolve(Os::Windows, "C:\\Users\\ana", &pairs, &fs, program);
        assert_eq!(found, Some(PathBuf::from(file)), "{program}");
    }
}

#[test]
fn windows_pathext_decides_the_extensions_tried() {
    let fs = FakeFs::default().program("C:\\tools\\npx.cmd");
    let pairs = [("PATH", "C:\\tools"), ("PATHEXT", ".EXE")];
    assert_eq!(
        resolve(Os::Windows, "C:\\Users\\ana", &pairs, &fs, "npx"),
        None
    );

    let fs = FakeFs::default().program("C:\\tools\\npx.exe");
    assert_eq!(
        resolve(Os::Windows, "C:\\Users\\ana", &pairs, &fs, "npx"),
        Some(PathBuf::from("C:\\tools\\npx.exe"))
    );
}

#[test]
fn windows_name_with_an_extension_is_tried_as_written() {
    let fs = FakeFs::default().program("C:\\tools\\tool.exe");
    let pairs = [("PATH", "C:\\tools")];
    assert_eq!(
        resolve(Os::Windows, "C:\\Users\\ana", &pairs, &fs, "tool.exe"),
        Some(PathBuf::from("C:\\tools\\tool.exe"))
    );
    assert_eq!(
        resolve(Os::Windows, "C:\\Users\\ana", &pairs, &fs, "tool.bat"),
        None
    );
}

#[test]
fn windows_path_splits_on_semicolons_and_drops_quotes() {
    let fs = FakeFs::default().program("C:\\Program Files\\Tool\\tool.exe");
    let pairs = [("PATH", "\"C:\\Program Files\\Tool\";C:\\other")];
    assert_eq!(
        resolve(Os::Windows, "C:\\Users\\ana", &pairs, &fs, "tool"),
        Some(PathBuf::from("C:\\Program Files\\Tool\\tool.exe"))
    );
}

#[test]
fn windows_absolute_path_without_extension_gets_pathext() {
    let fs = FakeFs::default().program("C:\\Program Files\\nodejs\\npx.cmd");
    assert_eq!(
        resolve(
            Os::Windows,
            "C:\\Users\\ana",
            &[],
            &fs,
            "C:\\Program Files\\nodejs\\npx"
        ),
        Some(PathBuf::from("C:\\Program Files\\nodejs\\npx.cmd"))
    );
    assert_eq!(
        resolve(Os::Windows, "C:\\Users\\ana", &[], &fs, "C:\\nowhere\\npx"),
        None
    );
}

#[test]
fn windows_relative_paths_with_separators_give_none() {
    let fs = FakeFs::default().program("C:\\tools\\npx.cmd");
    let pairs = [("PATH", "C:\\tools")];
    for program in [".\\npx", "tools\\npx", "./npx", "..\\npx"] {
        assert_eq!(
            resolve(Os::Windows, "C:\\Users\\ana", &pairs, &fs, program),
            None,
            "{program}"
        );
    }
}

#[test]
fn windows_does_not_search_mac_folders() {
    let fs = FakeFs::default().program("/opt/homebrew/bin/tool");
    assert_eq!(
        resolve(
            Os::Windows,
            "C:\\Users\\ana",
            &[("PATH", "/usr/bin")],
            &fs,
            "tool"
        ),
        None
    );
}
