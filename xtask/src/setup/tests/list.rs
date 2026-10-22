//! Tests of `setup --list`: the line format on fake pins, written out here, one line per tool and
//! per asset of the real pins, and that `--list` installs nothing and takes no other flag.

use super::*;
use clap::Parser;

/// What `list_with` prints for `pins`.
fn listed(pins: &Pins<'_>) -> String {
    let mut out = Printer::new(Vec::new(), false);
    list_with(&mut out, pins);
    String::from_utf8(out.into_inner()).unwrap()
}

#[test]
fn list_prints_a_tab_separated_line_per_tool_then_per_pinned_asset() {
    let tools = [
        Tool {
            name: "cargo-a",
            version: "1.2.3",
        },
        Tool {
            name: "cargo-b",
            version: "4.5.6",
        },
    ];
    let tool = fake_tool(Packing::Bare);
    let two_assets = Binary {
        name: "othertool",
        sha256: &[
            AssetSha256 {
                os: "macos",
                arch: "aarch64",
                sha256: "aa",
            },
            AssetSha256 {
                os: "windows",
                arch: "x86_64",
                sha256: "bb",
            },
        ],
        ..fake_tool(Packing::TarGz)
    };
    let binaries = [tool, two_assets];
    let printed = listed(&Pins {
        tools: &tools,
        binaries: &binaries,
    });
    assert_eq!(
        printed,
        format!(
            "tool\tcargo-a\t1.2.3\n\
             tool\tcargo-b\t4.5.6\n\
             binary\tfaketool\tfake-version\tlinux\tx86_64\t{ASSET_SHA256}\n\
             binary\tothertool\tfake-version\tmacos\taarch64\taa\n\
             binary\tothertool\tfake-version\twindows\tx86_64\tbb\n"
        )
    );
}

/// `setup --list` on the real pins: every tool and every pinned asset, Windows x86-64 included,
/// once each, and it exits with success.
#[test]
fn list_prints_every_real_pin_once() {
    let mut out = Printer::new(Vec::new(), false);
    assert_eq!(list(&mut out), ExitCode::SUCCESS);
    let printed = String::from_utf8(out.into_inner()).unwrap();
    let mut expected: Vec<String> = (TOOLS.iter())
        .map(|t| format!("tool\t{}\t{}", t.name, t.version))
        .collect();
    for b in BINARIES {
        for pin in b.sha256 {
            expected.push(format!(
                "binary\t{}\t{}\t{}\t{}\t{}",
                b.name, b.version, pin.os, pin.arch, pin.sha256
            ));
        }
    }
    assert_eq!(printed.lines().collect::<Vec<_>>(), expected);
    for name in ["gitleaks", "lefthook"] {
        let windows = binary(name)
            .sha256
            .iter()
            .find(|p| p.os == "windows")
            .unwrap();
        assert!(
            printed.contains(&format!("\twindows\tx86_64\t{}\n", windows.sha256)),
            "{printed}"
        );
    }
}

#[test]
fn list_parses_alone_and_refuses_binaries_and_only() {
    let cli = crate::Cli::try_parse_from(["xtask", "setup", "--list"]).unwrap();
    let crate::Commands::Setup {
        list,
        binaries,
        only,
    } = cli.command
    else {
        panic!("not setup");
    };
    assert!(list && !binaries && only.is_empty());
    for other in [&["--binaries"][..], &["--only", "gitleaks"]] {
        let args = ["xtask", "setup", "--list"].iter().chain(other);
        let refused = crate::Cli::try_parse_from(args).err().unwrap();
        assert_eq!(refused.kind(), clap::error::ErrorKind::ArgumentConflict);
    }
    let plain = crate::Cli::try_parse_from(["xtask", "setup"]).unwrap();
    assert!(matches!(
        plain.command,
        crate::Commands::Setup { list: false, .. }
    ));
}
