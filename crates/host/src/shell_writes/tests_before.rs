//! Ordinary commands, well inside the reader's depth and size caps
//! (`pre_write_guard::words::MAX_NESTING`, `MAX_NESTED_BYTES`), read as they did before the caps
//! by every reader: the shared reader under each escape at the guard's word limits and none (its
//! shell targets, held-file names and read-only checks), the data folder fence's
//! (`powershell_words`) and this module's `PowerShell` one.

use std::fmt::Write as _;

use crate::pre_write_guard::{Escape, Segments, powershell_words, shell_segments};

/// Real commands: `PowerShell` with `$(…)`, here-strings and arrays, Bash with a heredoc and
/// `$(…)`, cmd.
const CORPUS: &[&str] = &[
    r#"Get-ChildItem -Path "$($env:USERPROFILE)\Documents" -Filter *.docx | Select-Object -First 5"#,
    r#"$out = "Report-$(Get-Date -Format yyyyMMdd).docx"; Copy-Item 'Draft.docx' $out"#,
    "Set-Content -Path notes.md -Value @'\nLine one\nLine two\n'@",
    "Set-Content notes.md @\"\nHello $(whoami)\n\"@",
    r#"Remove-Item -Path 'a.txt','b.txt', "c d.txt""#,
    r#"Write-Host "Count: $( (Get-ChildItem *.pdf).Count )""#,
    r#"$files = @('Contract.docx', 'Memo.docx'); foreach ($f in $files) { Copy-Item $f "Backup\$f" }"#,
    r#"pandoc "Memo $(Get-Date -Format 'yyyy-MM-dd').md" -o "Memo.docx""#,
    r#"Write-Output "$(Join-Path $(Get-Location) $(Split-Path -Leaf $(Resolve-Path .\Memo.docx)))""#,
    r#"Set-Content out.txt "$(Get-Content a.txt)"; Copy-Item out.txt "$(Join-Path $(Get-Location) 'Archive')""#,
    "cat > notes.md <<'EOF'\n# Notes\nSee $(date)\nEOF",
    r#"mkdir -p "out/$(date +%F)" && pandoc memo.md -o "out/$(date +%F)/memo.docx""#,
    r#"for f in *.md; do pandoc "$f" -o "${f%.md}.docx"; done"#,
    r#"bash -lc 'cd Drafts && sed -i "s/foo/bar/" a.txt'"#,
    r#"pwsh -Command "Get-Content '$(pwd)/a.txt' | Set-Content b.txt""#,
    r#"cmd /c "copy Draft.docx Final.docx && del Draft.docx""#,
    r#"cmd.exe /c del /q "Old Memo.docx""#,
    r#"cmd /c "for %f in (*.txt) do type %f > %f.bak""#,
];

/// FNV-1a, 64 bits: a fixed hash of a reading's text.
fn fnv(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// Every reading of `command` a caller makes, as text.
fn readings(command: &str) -> String {
    let mut out = String::new();
    for escape in [Escape::Backslash, Escape::Backtick] {
        for limit in [64, 4096, usize::MAX] {
            let mut segments = Segments::new(limit);
            shell_segments(command, escape, &mut segments);
            let list: Vec<_> = segments.iter().collect();
            let unsure = segments.unsure();
            writeln!(out, "{escape:?} {limit} {unsure} {list:?}").unwrap();
        }
    }
    writeln!(out, "{:?}", powershell_words(command)).unwrap();
    writeln!(out, "{:?}", super::powershell::segments(command)).unwrap();
    out
}

/// The hash of [`readings`] of each of [`CORPUS`], as a5803b775 read them.
const BEFORE: &[u64] = &[
    0x27f7_1e42_a43d_a61b,
    0xd57e_13bb_c266_566b,
    0x5f0d_0a57_5998_1874,
    0x3245_8250_8673_9242,
    0x419b_5d49_1f01_2c0a,
    0xd90b_c95d_5914_d354,
    0xe374_a352_f52a_e4e3,
    0xf898_1830_7c07_3804,
    0x2695_8acb_100d_063c,
    0x42b3_da7e_bf90_7f98,
    0x8ab8_2d28_236b_c1b7,
    0x1edd_6942_de14_becd,
    0x8196_c03f_d416_bf64,
    0x7278_cde3_11ed_ca55,
    0xc7fc_ee26_232a_b809,
    0x92d5_9e11_3359_07ed,
    0xec63_d83b_63ef_788d,
    0x0eeb_1f80_e6e9_4d89,
];

#[test]
fn ordinary_commands_read_as_before_by_every_reader() {
    let got: Vec<u64> = CORPUS
        .iter()
        .map(|command| fnv(&readings(command)))
        .collect();
    for ((command, got), want) in CORPUS.iter().zip(&got).zip(BEFORE) {
        assert_eq!(got, want, "{command}\n{}", readings(command));
    }
    assert_eq!(got.len(), BEFORE.len(), "{got:#x?}");
}
