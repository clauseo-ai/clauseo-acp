//! The differential test: PowerShell's own parser and parameter binder read each line the
//! allowlist accepts, and its rewrite, as the allowlist does. It runs every PowerShell it finds:
//! Windows PowerShell 5.1 (`%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe`, on
//! Windows) and PowerShell 7 (the program [`PWSH_VAR`] names, on any OS; else, on Windows, its
//! usual `%ProgramFiles%\PowerShell\7\pwsh.exe`). With none it passes, saying it skipped; with
//! `CLAUSEO_PS_DIFF_REQUIRE=1` set it fails instead (a laptop row). CI's Windows job
//! (`windows-pr-tests.yml`, the workspace nextest on windows-2025, which has both) runs it on
//! every pull request that gets the `ready` label.
//!
//! For each line PowerShell must find: no parse error; only bare words, single- or
//! double-quoted strings with nothing expanded, parameters, integer counts and Select-Object's
//! array of bare words as command elements; no comment, variable, continuation or other token;
//! and the same bound parameters and values as the allowlist reads (`StaticParameterBinder`; a
//! provider's dynamic switch, which it cannot bind, counts when its name was written). Each
//! PowerShell found must answer the same.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::parse::{Count, Kind, Line, PIN, Source, parse, quoted, rewrite};

/// The script each PowerShell runs on the lines in `CLAUSEO_PS_CORPUS` (a JSON array).
const SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
$L = 'System.Management.Automation.Language'
$okKinds = @('Generic', 'Identifier', 'Parameter', 'StringLiteral', 'StringExpandable', 'Number',
    'Comma', 'Pipe', 'Semi', 'EndOfInput')
function Text($ast) {
    if ($null -eq $ast -or $ast -is "$L.CommandParameterAst") { return 'True' }
    if ($ast -is "$L.ArrayLiteralAst") { return (@($ast.Elements | ForEach-Object { $_.Value }) -join ',') }
    return [string]$ast.Value
}
function Plain($ce) {
    if ($ce -is "$L.CommandParameterAst") { return $null -eq $ce.Argument }
    if ($ce -is "$L.StringConstantExpressionAst") { return $true }
    if ($ce -is "$L.ConstantExpressionAst") { return $ce.StaticType -eq [int] }
    if ($ce -is "$L.ArrayLiteralAst") {
        foreach ($e in $ce.Elements) {
            if ($e -isnot "$L.StringConstantExpressionAst" -or $e.StringConstantType -ne 'BareWord') { return $false }
        }
        return $true
    }
    return $false
}
$items = ConvertFrom-Json $env:CLAUSEO_PS_CORPUS
$out = foreach ($line in $items) {
    $tokens = $null; $errors = $null
    $ast = [System.Management.Automation.Language.Parser]::ParseInput($line, [ref]$tokens, [ref]$errors)
    $bad = @()
    foreach ($e in $errors) { $bad += 'error:' + $e.ErrorId }
    foreach ($t in $tokens) {
        if ($okKinds -notcontains [string]$t.Kind) { $bad += 'token:' + $t.Kind }
        if ($t -is "$L.StringExpandableToken" -and $t.NestedTokens) { $bad += 'expanded' }
    }
    $commands = @()
    $pipelines = $ast.FindAll({ param($a) $a -is [System.Management.Automation.Language.PipelineAst] }, $true)
    foreach ($p in $pipelines) {
        foreach ($el in $p.PipelineElements) {
            if ($el -isnot "$L.CommandAst" -or $el.InvocationOperator -ne 'Unknown') { $bad += 'not a command'; continue }
            foreach ($ce in @($el.CommandElements | Select-Object -Skip 1)) {
                if (-not (Plain $ce)) { $bad += 'element:' + $ce.GetType().Name }
            }
            $binding = [System.Management.Automation.Language.StaticParameterBinder]::BindCommand($el, $true)
            $bound = [ordered]@{}
            foreach ($k in @($binding.BoundParameters.Keys | Sort-Object)) {
                $bound[$k.ToLowerInvariant()] = Text $binding.BoundParameters[$k].Value
            }
            $unbound = @($binding.BindingExceptions.Keys | ForEach-Object { $_.ToLowerInvariant() } | Sort-Object)
            $commands += [ordered]@{ name = $el.GetCommandName(); bound = $bound; unbound = $unbound }
        }
    }
    [ordered]@{ line = $line; bad = $bad; commands = $commands }
}
ConvertTo-Json -InputObject @($out) -Depth 8 -Compress
"#;

/// A provider's dynamic switches, which the static binder cannot bind.
const DYNAMIC: [&str; 3] = ["file", "directory", "raw"];

/// The accepted lines, with this OS's paths and aliases (Unix's PowerShell has no `ls`, `cat`,
/// `dir` or `type` alias).
fn corpus() -> Vec<String> {
    let (m, sep) = if cfg!(windows) {
        (r"C:\m", '\\')
    } else {
        ("/m", '/')
    };
    let mut lines = vec![
        format!(
            r#"Get-ChildItem -Recurse -File "{m}" | Select-Object FullName, Length; Get-ChildItem "{m}{sep}notes""#
        ),
        "Get-ChildItem".to_owned(),
        format!("gci -Recurse -Directory -Force -Name {m}"),
        format!("gc '{m}{sep}Contract (final).docx' -TotalCount 40"),
        format!("Get-Content -LiteralPath '{m}{sep}memo [v2].md' -Raw"),
        format!(r#"gc "{m}{sep}O'Brien; #1.txt" -Tail 3 | select -First 5"#),
        format!(
            "gc .{sep}a.txt -Head 007; gci -Path {m}{sep}Drafts | Select-Object -Property Name,Length -First 2"
        ),
    ];
    if cfg!(windows) {
        lines.extend([
            r"ls C:\m".to_owned(),
            r"dir 'C:\m' -File".to_owned(),
            r"cat C:\m\a.txt -Last 1".to_owned(),
            r"type C:\m\a.txt -First 2".to_owned(),
        ]);
    }
    lines
}

/// One command as read: its path (when it names one) and every other bound parameter,
/// lowercase, with its value as PowerShell's text.
type Read = (Option<String>, BTreeMap<String, String>);

/// What the allowlist reads each command of `line` as, each with whether it is a source.
fn expected(line: &Line) -> Vec<(bool, Read)> {
    let mut out = Vec::new();
    let on = |map: &mut BTreeMap<String, String>, name: &str, set: bool| {
        if set {
            map.insert(name.to_owned(), "True".to_owned());
        }
    };
    for statement in &line.statements {
        let mut params = BTreeMap::new();
        match &statement.source {
            Source::List {
                recurse,
                kind,
                force,
                name,
                ..
            } => {
                on(&mut params, "recurse", *recurse);
                on(&mut params, "file", *kind == Some(Kind::File));
                on(&mut params, "directory", *kind == Some(Kind::Directory));
                on(&mut params, "force", *force);
                on(&mut params, "name", *name);
            }
            Source::Read { count, raw, .. } => {
                let count = match count {
                    Some(Count::Total(n)) => Some(("totalcount", n)),
                    Some(Count::Tail(n)) => Some(("tail", n)),
                    None => None,
                };
                if let Some((name, n)) = count {
                    params.insert(name.to_owned(), n.to_string());
                }
                on(&mut params, "raw", *raw);
            }
        }
        out.push((
            true,
            (
                statement.source.path().map(|path| path.text.clone()),
                params,
            ),
        ));
        if let Some(select) = &statement.select {
            let mut params = BTreeMap::new();
            if !select.properties.is_empty() {
                params.insert("property".to_owned(), select.properties.join(","));
            }
            if let Some(n) = select.first {
                params.insert("first".to_owned(), n.to_string());
            }
            out.push((false, (None, params)));
        }
    }
    out
}

/// PowerShell's answer for one command, in the same form as [`expected`]'s; the path's text
/// with `/` as `\` on Windows; and whether it was given as `-LiteralPath`.
fn answered(command: &Value) -> (Option<String>, BTreeMap<String, String>, bool) {
    let mut params: BTreeMap<String, String> = command["bound"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(key, value)| (key.clone(), value.as_str().unwrap_or_default().to_owned()))
        .collect();
    for name in command["unbound"].as_array().unwrap() {
        let name = name.as_str().unwrap();
        assert!(
            DYNAMIC.contains(&name),
            "{name} could not be bound: {command}"
        );
        params.insert(name.to_owned(), "True".to_owned());
    }
    let literal = params.contains_key("literalpath");
    let path = params
        .remove("path")
        .or_else(|| params.remove("literalpath"))
        .map(|path| {
            if cfg!(windows) {
                path.replace('/', "\\")
            } else {
                path
            }
        });
    (path, params, literal)
}

/// The variable naming a PowerShell 7 program (`pwsh`) to compare with, on any OS.
const PWSH_VAR: &str = "CLAUSEO_PS_DIFF_PWSH";

/// The PowerShell programs on this machine (module doc).
fn shells() -> Vec<PathBuf> {
    let mut found = Vec::new();
    if cfg!(windows)
        && let Some(root) = std::env::var_os("SystemRoot")
    {
        let five = PathBuf::from(root).join(r"System32\WindowsPowerShell\v1.0\powershell.exe");
        if five.is_file() {
            found.push(five);
        }
    }
    let named = std::env::var_os(PWSH_VAR).map(PathBuf::from);
    let installed = std::env::var_os("ProgramFiles")
        .filter(|_| cfg!(windows))
        .map(|root| PathBuf::from(root).join(r"PowerShell\7\pwsh.exe"));
    found.extend(named.or(installed).filter(|seven| seven.is_file()));
    found
}

/// `shell`'s answers for `lines`.
#[expect(
    clippy::disallowed_methods,
    reason = "a test runs the real PowerShell once to compare its parser with the allowlist's"
)]
fn ask(shell: &Path, lines: &[String]) -> Vec<Value> {
    let dir = std::env::temp_dir().join(format!("clauseo-ps-diff-{}", std::process::id()));
    crate::write::create_dir_all(&dir).unwrap();
    let script = dir.join("diff.ps1");
    crate::write::write_file(&script, SCRIPT.as_bytes()).unwrap();
    let output = std::process::Command::new(shell)
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(&script)
        .env("CLAUSEO_PS_CORPUS", serde_json::to_string(lines).unwrap())
        .env("DOTNET_EnableWriteXorExecute", "0")
        .output()
        .unwrap();
    crate::write::remove_dir_all(&dir).unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{}: {stdout} {}",
        shell.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_str::<Vec<Value>>(stdout.trim()).unwrap()
}

#[test]
fn powershell_reads_each_accepted_line_and_its_rewrite_as_the_allowlist_does() {
    let shells = shells();
    if shells.is_empty() {
        assert!(
            std::env::var_os("CLAUSEO_PS_DIFF_REQUIRE").is_none(),
            "CLAUSEO_PS_DIFF_REQUIRE is set and no PowerShell was found"
        );
        eprintln!("skipped: no PowerShell on this machine");
        return;
    }
    let canonical = if cfg!(windows) { r"C:\m\x" } else { "/m/x" };
    let mut lines = Vec::new();
    let mut wants = Vec::new();
    for line in corpus() {
        let parsed = parse(&line, cfg!(windows)).unwrap_or_else(|| panic!("refused: {line}"));
        let paths = vec![quoted(canonical).unwrap(); parsed.statements.len()];
        let command = rewrite(&parsed, &paths).unwrap();
        let tail = command.strip_prefix(PIN).unwrap().to_owned();
        let read = expected(&parsed);
        let want: Vec<Read> = read.iter().map(|(_, read)| read.clone()).collect();
        let rewritten: Vec<Read> = read
            .iter()
            .map(|(source, (_, params))| (source.then(|| canonical.to_owned()), params.clone()))
            .collect();
        lines.extend([line, tail]);
        wants.extend([(want, false), (rewritten, true)]);
    }
    let mut answers = Vec::new();
    for shell in &shells {
        let got = ask(shell, &lines);
        assert_eq!(got.len(), lines.len(), "{}", shell.display());
        for ((answer, (want, rewritten)), line) in got.iter().zip(&wants).zip(&lines) {
            assert_eq!(
                answer["bad"],
                serde_json::json!([]),
                "{}: {line}",
                shell.display()
            );
            let commands = answer["commands"].as_array().unwrap();
            let read: Vec<_> = commands.iter().map(answered).collect();
            let plain: Vec<_> = read
                .iter()
                .map(|(path, params, _)| (path.clone(), params.clone()))
                .collect();
            assert_eq!(&plain, want, "{}: {line}", shell.display());
            if *rewritten {
                let qualified = commands.iter().all(|command| {
                    let name = command["name"].as_str().unwrap();
                    name.starts_with(r"Microsoft.PowerShell.Management\")
                        || name == r"Microsoft.PowerShell.Utility\Select-Object"
                });
                assert!(qualified, "{line}");
                assert!(
                    read.iter()
                        .all(|(path, _, literal)| path.is_none() || *literal),
                    "{line}"
                );
            }
        }
        let read: Vec<Vec<_>> = got
            .iter()
            .map(|answer| {
                answer["commands"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(answered)
                    .collect()
            })
            .collect();
        answers.push(read);
    }
    if let [first, rest @ ..] = answers.as_slice() {
        for other in rest {
            assert_eq!(first, other, "5.1 and 7 bind differently");
        }
    }
}
