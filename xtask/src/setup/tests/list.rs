//! Tests of `setup --list`: both formats on fake pins, written out here; the real pins, one line
//! each; a pin with no asset fails on stderr; and `--list` parses alone, with its format, and
//! takes no other flag.

use super::*;
use crate::setup::list::list_with;
use clap::Parser;

/// Whether `list_with` passed for `pins` in `format`, and what it printed on stdout and stderr.
fn listed(pins: &Pins<'_>, format: ListFormat) -> (bool, String, String) {
    let mut out = Printer::new(Vec::new(), false);
    let mut err = Printer::new(Vec::new(), false);
    let ok = list_with(&mut out, &mut err, pins, format);
    let text = |p: Printer<Vec<u8>>| String::from_utf8(p.into_inner()).unwrap();
    (ok, text(out), text(err))
}

/// What `list` printed on the real pins in `format`, after checking it passed with no stderr.
fn listed_real(format: ListFormat) -> String {
    let mut out = Printer::new(Vec::new(), false);
    let mut err = Printer::new(Vec::new(), false);
    assert_eq!(list(&mut out, &mut err, format), ExitCode::SUCCESS);
    assert_eq!(String::from_utf8(err.into_inner()).unwrap(), "");
    String::from_utf8(out.into_inner()).unwrap()
}

const FAKE_TOOLS: [Tool; 2] = [
    Tool {
        name: "cargo-a",
        version: "1.2.3",
    },
    Tool {
        name: "cargo-b",
        version: "4.5.6",
    },
];

/// The fake tool, and one pinned on two platforms, held as a `.tar.gz`.
fn fake_binaries() -> [Binary; 2] {
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
    [fake_tool(Packing::Bare), two_assets]
}

const URL: &str = "https://github.com/example/faketool/releases/download/vfake-version";

#[test]
fn list_prints_a_tab_separated_line_per_tool_then_per_pinned_asset() {
    let binaries = fake_binaries();
    let pins = Pins {
        tools: &FAKE_TOOLS,
        binaries: &binaries,
    };
    let (ok, printed, err) = listed(&pins, ListFormat::Text);
    assert!(ok && err.is_empty(), "{err}");
    assert_eq!(
        printed,
        format!(
            "tool\tcargo-a\t1.2.3\n\
             tool\tcargo-b\t4.5.6\n\
             binary\tfaketool\tfake-version\tlinux\tx86_64\t{ASSET_SHA256}\t\
             {URL}/faketool_fake-version_linux_x86_64\n\
             binary\tothertool\tfake-version\tmacos\taarch64\taa\t\
             {URL}/faketool_fake-version_macos_aarch64\n\
             binary\tothertool\tfake-version\twindows\tx86_64\tbb\t\
             {URL}/faketool_fake-version_windows_x86_64\n"
        )
    );
}

#[test]
fn list_json_prints_one_object_with_the_same_pins() {
    let binaries = fake_binaries();
    let pins = Pins {
        tools: &FAKE_TOOLS,
        binaries: &binaries,
    };
    let (ok, printed, err) = listed(&pins, ListFormat::Json);
    assert!(ok && err.is_empty(), "{err}");
    assert_eq!(printed.lines().count(), 1, "{printed}");
    let asset = |name: &str, os: &str, arch: &str, sha256: &str| {
        let file = format!("faketool_fake-version_{os}_{arch}");
        serde_json::json!({
            "name": name, "version": "fake-version", "os": os, "arch": arch, "sha256": sha256,
            "asset": file, "url": format!("{URL}/{file}"),
        })
    };
    let expected = serde_json::json!({
        "tools": [
            { "name": "cargo-a", "version": "1.2.3" },
            { "name": "cargo-b", "version": "4.5.6" },
        ],
        "binaries": [
            asset("faketool", "linux", "x86_64", ASSET_SHA256),
            asset("othertool", "macos", "aarch64", "aa"),
            asset("othertool", "windows", "x86_64", "bb"),
        ],
    });
    let parsed: serde_json::Value = serde_json::from_str(&printed).unwrap();
    assert_eq!(parsed, expected);
}

#[test]
fn list_with_no_pins_prints_nothing_as_text() {
    let pins = Pins {
        tools: &[],
        binaries: &[],
    };
    assert_eq!(
        listed(&pins, ListFormat::Text),
        (true, String::new(), String::new())
    );
}

#[test]
fn a_pin_with_no_release_asset_fails_on_stderr_and_lists_nothing() {
    let unnamed = Binary {
        asset: |_, _, _| None,
        ..fake_tool(Packing::Bare)
    };
    let binaries = [unnamed];
    let pins = Pins {
        tools: &FAKE_TOOLS,
        binaries: &binaries,
    };
    for format in [ListFormat::Text, ListFormat::Json] {
        let (ok, printed, err) = listed(&pins, format);
        assert!(!ok);
        assert_eq!(printed, "");
        assert_eq!(
            err,
            "setup --list: faketool pins a sha256 for linux-x86_64, but names no release asset \
             there\n    fix: return that asset's file name from faketool's `asset` function in \
             xtask/src/setup.rs, or remove the pin\n"
        );
    }
}

/// `setup --list` on the real pins exits with success, prints nothing on stderr, and prints a
/// line per tool and per pin, none twice, with a Windows x86-64 line for each binary; the JSON has
/// an entry per pin, with one Windows x86-64 entry per binary.
#[test]
fn list_prints_every_real_pin_once() {
    let printed = listed_real(ListFormat::Text);
    let lines: Vec<&str> = printed.lines().collect();
    let pins: usize = BINARIES.iter().map(|b| b.sha256.len()).sum();
    assert_eq!(lines.len(), TOOLS.len() + pins, "{printed}");
    let mut unique = lines.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), lines.len(), "{printed}");
    for b in BINARIES {
        let windows = format!("binary\t{}\t{}\twindows\tx86_64\t", b.name, b.version);
        assert!(lines.iter().any(|l| l.starts_with(&windows)), "{printed}");
    }
    let json: serde_json::Value = serde_json::from_str(&listed_real(ListFormat::Json)).unwrap();
    let entries = json["binaries"].as_array().unwrap();
    assert_eq!(entries.len(), pins);
    for b in BINARIES {
        let windows = (entries.iter())
            .filter(|e| e["name"] == b.name && e["os"] == "windows" && e["arch"] == "x86_64");
        assert_eq!(windows.count(), 1, "{}", b.name);
    }
}

#[test]
fn list_takes_an_optional_format_and_no_other_flag() {
    let parse = |args: &[&str]| crate::Cli::try_parse_from(["xtask", "setup"].iter().chain(args));
    for (args, format) in [
        (&["--list"][..], ListFormat::Text),
        (&["--list=text"], ListFormat::Text),
        (&["--list=json"], ListFormat::Json),
    ] {
        let crate::Commands::Setup { list, .. } = parse(args).unwrap().command else {
            panic!("not setup");
        };
        assert_eq!(list, Some(format), "{args:?}");
    }
    let refused: [&[&str]; 4] = [
        &["--list", "--binaries"],
        &["--binaries", "--list"],
        &["--list", "--only", "gitleaks"],
        &["--list=json", "--only", "gitleaks"],
    ];
    for args in refused {
        let error = parse(args).err().unwrap();
        assert_eq!(
            error.kind(),
            clap::error::ErrorKind::ArgumentConflict,
            "{args:?}"
        );
    }
    // A format only follows `=`, so `--list json` is not read as one.
    assert!(parse(&["--list", "json"]).is_err());
    let crate::Commands::Setup { list, .. } = parse(&[]).unwrap().command else {
        panic!("not setup");
    };
    assert_eq!(list, None);
}
