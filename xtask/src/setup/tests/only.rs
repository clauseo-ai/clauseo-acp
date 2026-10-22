//! Tests of `setup --only`: which tools a selection installs and checks, an unknown name, `--only`
//! with `--binaries`, and that no `--only` selects everything. Through an injected runner and the
//! fake tool, never a real install.

use super::*;
use crate::scratch::Scratch;
use clap::Parser;

/// What `run_with` ran, as shell-like text, for `selection` of `TOOLS` and `tool`, where `tool`'s
/// `version` prints each entry of `versions` in turn (`None`: it cannot start).
pub(super) fn run_selected(
    machine: &Machine<'_>,
    tool: &Binary,
    selection: Selection<'_>,
    install: bool,
    versions: &[Option<&str>],
) -> (bool, Vec<String>, String) {
    let mut ran = Vec::new();
    let mut versions = versions.iter();
    let mut runner = |cmd: &Cmd, at: &Path, _: Output| -> io::Result<Ran> {
        ran.push(cmd.to_string());
        match cmd.program {
            Program::Named("curl") => {
                // The run lock is held while the binaries install, unless it could not be taken.
                let lock = at.parent().unwrap().join(".xtask-setup.lock");
                if lock.is_file() {
                    let taken = std::fs::File::open(&lock)?.try_lock();
                    assert!(matches!(taken, Err(std::fs::TryLockError::WouldBlock)));
                }
                Scratch::write(at.join(curl_output(cmd)), ASSET_BYTES)?;
            }
            Program::Named("faketool") => {
                return match versions.next().expect("a version for each check") {
                    Some(version) => exited(true, &format!("{version}\n"), ""),
                    None => Err(io::Error::new(io::ErrorKind::NotFound, "not found")),
                };
            }
            _ => {}
        }
        exited(true, "", "")
    };
    let mut out = Printer::new(Vec::new(), false);
    let pins = Pins {
        tools: TOOLS,
        binaries: std::slice::from_ref(tool),
    };
    let ok = run_with(
        &mut out,
        machine,
        &pins,
        selection,
        install,
        Path::new("."),
        &mut runner,
    );
    let printed = String::from_utf8(out.into_inner()).unwrap();
    (ok, ran, printed)
}

fn names(names: &[&str]) -> Vec<String> {
    names.iter().map(|&n| n.to_owned()).collect()
}

/// The `cargo install` command for the tool in `TOOLS` named `name`.
fn cargo_install(name: &str) -> String {
    let tool = TOOLS.iter().find(|t| t.name == name).unwrap();
    format!("cargo install --locked {name} --version {}", tool.version)
}

/// `setup --only a --only b --binaries` parses to both names, in order, and the flag.
#[test]
fn only_is_repeatable_and_combines_with_binaries() {
    let cli = crate::Cli::try_parse_from([
        "xtask",
        "setup",
        "--only",
        "cargo-nextest",
        "--binaries",
        "--only",
        "gitleaks",
    ])
    .unwrap();
    let crate::Commands::Setup { binaries, only, .. } = cli.command else {
        panic!("not setup");
    };
    assert!(binaries);
    assert_eq!(only, names(&["cargo-nextest", "gitleaks"]));
    let plain = crate::Cli::try_parse_from(["xtask", "setup"]).unwrap();
    let crate::Commands::Setup { only, .. } = plain.command else {
        panic!("not setup");
    };
    assert_eq!(Selection::of(&only), Selection::All);
}

#[test]
fn only_values_select_those_names_and_none_select_all() {
    let picked = names(&["cargo-nextest"]);
    assert_eq!(Selection::of(&picked), Selection::Only(&picked));
    assert_eq!(Selection::of(&[]), Selection::All);
}

#[test]
fn only_a_cargo_tool_installs_just_that_tool_and_checks_no_binary() {
    let dir = Scratch::new("setup-only-tool");
    let tool = fake_tool(Packing::Bare);
    let picked = names(&["cargo-nextest"]);
    let (ok, ran, printed) = run_selected(
        &linux(&dir.join("bin"), true),
        &tool,
        Selection::Only(&picked),
        false,
        &[],
    );
    assert!(ok, "{printed}");
    assert_eq!(ran, [cargo_install("cargo-nextest")]);
    assert!(!printed.contains("faketool"), "{printed}");
}

/// Names are found wherever they sit in `TOOLS`, run in `TOOLS` order, and a binary by its name.
#[test]
fn only_several_names_sets_up_each_in_pin_order() {
    let dir = Scratch::new("setup-only-several");
    let tool = fake_tool(Packing::Bare);
    let picked = names(&["faketool", "cargo-mutants", "cargo-deny", "cargo-deny"]);
    let (ok, ran, printed) = run_selected(
        &linux(&dir.join("bin"), true),
        &tool,
        Selection::Only(&picked),
        false,
        &[Some(tool.version)],
    );
    assert!(ok, "{printed}");
    assert_eq!(
        ran,
        [
            cargo_install("cargo-deny"),
            cargo_install("cargo-mutants"),
            "faketool version".to_owned(),
        ]
    );
    assert!(
        printed.contains("faketool version fake-version ok\n"),
        "{printed}"
    );
}

#[test]
fn an_unknown_name_fails_before_anything_runs_and_lists_the_pinned_names() {
    let dir = Scratch::new("setup-only-unknown");
    let tool = fake_tool(Packing::Bare);
    let picked = names(&[
        "cargo-nextest",
        "nextest",
        "faketool",
        "nope",
        "gitleak",
        "nope",
    ]);
    let (ok, ran, printed) = run_selected(
        &linux(&dir.join("bin"), true),
        &tool,
        Selection::Only(&picked),
        true,
        &[],
    );
    assert!(!ok);
    assert!(ran.is_empty(), "{ran:?}");
    assert!(!dir.join("bin").exists());
    let valid = "cargo-nextest, cargo-deny, cargo-shear, typos-cli, taplo-cli, cargo-mutants, \
                 faketool";
    let fix = format!(
        "    fix: name a pinned tool after --only, one of: {valid}; then run cargo xtask setup \
         again\n"
    );
    assert_eq!(
        printed,
        format!(
            "setup --only nextest: no pinned tool is named nextest\n{fix}\
             setup --only nope: no pinned tool is named nope\n{fix}\
             setup --only gitleak: no pinned tool is named gitleak\n{fix}"
        )
    );
}

#[test]
fn only_a_binary_with_binaries_installs_just_that_binary() {
    let dir = Scratch::new("setup-only-binary");
    let bin = dir.join("bin");
    let tool = fake_tool(Packing::Bare);
    let v = tool.version;
    let picked = names(&["faketool"]);
    let (ok, ran, printed) = run_selected(
        &linux(&bin, true),
        &tool,
        Selection::Only(&picked),
        true,
        &[None, Some(v)],
    );
    assert!(ok, "{printed}");
    let asset = format!("faketool_{v}_linux_x86_64");
    let url = format!("https://github.com/example/faketool/releases/download/v{v}/{asset}");
    assert_eq!(
        ran,
        [
            "faketool version".to_owned(),
            expected_curl(&asset, &url),
            "faketool version".to_owned(),
        ]
    );
    assert_eq!(std::fs::read(bin.join("faketool")).unwrap(), ASSET_BYTES);
}

/// With `--binaries` but no binary selected, nothing touches cargo's bin: not even the run lock.
#[test]
fn only_a_cargo_tool_with_binaries_leaves_the_bin_alone() {
    let dir = Scratch::new("setup-only-tool-binaries");
    let bin = dir.join("bin");
    let tool = fake_tool(Packing::Bare);
    let picked = names(&["cargo-nextest"]);
    let (ok, ran, printed) = run_selected(
        &linux(&bin, true),
        &tool,
        Selection::Only(&picked),
        true,
        &[],
    );
    assert!(ok, "{printed}");
    assert_eq!(ran, [cargo_install("cargo-nextest")]);
    assert!(!bin.exists());
}

#[test]
fn without_only_every_tool_and_binary_is_set_up() {
    let dir = Scratch::new("setup-only-default");
    let tool = fake_tool(Packing::Bare);
    let (ok, ran, printed) = run_selected(
        &linux(&dir.join("bin"), true),
        &tool,
        Selection::of(&[]),
        false,
        &[Some(tool.version)],
    );
    assert!(ok, "{printed}");
    let mut expected = cargo_installs();
    expected.push("faketool version".to_owned());
    assert_eq!(ran, expected);
}
