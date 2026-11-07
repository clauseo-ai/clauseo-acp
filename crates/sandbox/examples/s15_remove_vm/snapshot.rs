//! `snapshot <file>` (a normal PowerShell, the lawyer): a sorted, tab-separated listing of what
//! `--remove` may touch and what it must not, for `compare`. One line per item:
//!
//! - `me <sid> <profile>`: who listed;
//! - `item <path> dir` or `item <path> file <size> <sha256>`: every folder and file under the
//!   matters and their state folders (a link is `link`, never followed);
//! - `acl.matter|acl.shared|acl.lawyer <path> <owner> <sddl>`: those items, `DeviceSync`, and the
//!   lawyer's profile folder, its `AppData` and each folder in that, owner, DACL and label;
//! - `user <name> <sid> <enabled>`, `group <name> <sid> <member sids>`: the local accounts;
//! - `profiledir <name>`: the folders beside the lawyer's profile (`C:\Users`);
//! - `record state present|absent|unreadable` and `recordvalue <line>` from `reg query`.

use std::path::{Path, PathBuf};

use base64::Engine as _;
use clauseo_sandbox::windows::sandbox_owner_sid;
use clauseo_sandbox::windows::setup::profile::profile_of;
use clauseo_sandbox::windows::sids::Sid;
use sha2::{Digest as _, Sha256};

use crate::prepare::device_sync;
use crate::{Out, ROOT, win};

/// Lowercase hex digits, for the hashes.
const HEX: &[u8; 16] = b"0123456789abcdef";

/// The machine's record, as the setup exe writes it.
pub(crate) const RECORD_KEY: &str = r"SOFTWARE\Clauseo\Sandbox";

/// Lists users and groups, each group with its members' SIDs, one tab-separated line each.
const ACCOUNTS: &str = r#"$ErrorActionPreference = 'Stop'
foreach ($u in Get-LocalUser | Sort-Object Name) { "user`t$($u.Name)`t$($u.SID.Value)`t$($u.Enabled)" }
foreach ($g in Get-LocalGroup | Sort-Object Name) {
  $m = try { (@(Get-LocalGroupMember -Group $g.Name -ErrorAction Stop) | ForEach-Object { $_.SID.Value } | Sort-Object) -join ',' } catch { 'unreadable' }
  "group`t$($g.Name)`t$($g.SID.Value)`t$m"
}"#;

/// The `snapshot` step.
pub(crate) fn snapshot(out: &mut Out, file: &Path) -> Result<(), String> {
    let mut lines = vec!["# s15_remove_vm snapshot".to_owned()];
    let me = win::current_user_sid()?;
    let sid = Sid::parse(&me).map_err(|e| e.to_string())?;
    let profile = profile_of(&sid)
        .map_err(|e| e.to_string())?
        .map(|p| p.path)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
        .ok_or("the lawyer's profile can't be found")?;
    lines.push(format!("me\t{me}\t{}", profile.display()));
    for root in ["matters", "state"] {
        walk(&Path::new(ROOT).join(root), &mut lines)?;
    }
    lines.push(acl_line("acl.shared", &device_sync()));
    lines.push(acl_line("acl.lawyer", &profile));
    let app_data = profile.join("AppData");
    lines.push(acl_line("acl.lawyer", &app_data));
    for entry in sorted_entries(&app_data)? {
        lines.push(acl_line("acl.lawyer", &entry));
    }
    if let Some(users) = profile.parent() {
        for entry in sorted_entries(users)? {
            let name = entry.file_name().unwrap_or_default().to_string_lossy();
            lines.push(format!("profiledir\t{name}\t"));
        }
    }
    lines.extend(accounts()?);
    lines.extend(record()?);
    lines.sort();
    lines.dedup();
    let mut text = lines.join("\n");
    text.push('\n');
    win::make_file(file, text.as_bytes())?;
    let count = |kind: &str| lines.iter().filter(|l| l.starts_with(kind)).count();
    out.evidence("items", &count("item\t").to_string());
    out.evidence("descriptors", &count("acl.").to_string());
    out.evidence("users", &count("user\t").to_string());
    out.evidence("groups", &count("group\t").to_string());
    let state = lines.iter().find(|l| l.starts_with("record\t"));
    out.evidence("record", state.map_or("?", String::as_str));
    out.check(true, &format!("listing written to {}", file.display()));
    Ok(())
}

/// The entries of the folder `dir`, sorted by path.
fn sorted_entries(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut paths = Vec::new();
    for entry in entries {
        paths.push(entry.map_err(|e| e.to_string())?.path());
    }
    paths.sort();
    Ok(paths)
}

/// `path`'s owner and SDDL under `kind`.
fn acl_line(kind: &str, path: &Path) -> String {
    let sddl = win::sddl_of(path).unwrap_or_else(|e| format!("unreadable ({e})"));
    let owner = sddl
        .strip_prefix("O:")
        .and_then(|rest| rest.split("D:").next())
        .unwrap_or("?")
        .to_owned();
    format!("{kind}\t{}\t{owner}\t{sddl}", path.display())
}

/// `path` and everything below it: content lines and descriptors.
fn walk(path: &Path, lines: &mut Vec<String>) -> Result<(), String> {
    let meta = std::fs::symlink_metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let shown = path.display();
    lines.push(acl_line("acl.matter", path));
    if meta.file_type().is_symlink() {
        lines.push(format!("item\t{shown}\tlink"));
        return Ok(());
    }
    if meta.is_dir() {
        lines.push(format!("item\t{shown}\tdir"));
        for entry in sorted_entries(path)? {
            walk(&entry, lines)?;
        }
        return Ok(());
    }
    let bytes = std::fs::read(path).map_err(|e| format!("{shown}: {e}"))?;
    let hash = Sha256::digest(&bytes)
        .iter()
        .fold(String::new(), |mut hex, b| {
            hex.push(char::from(HEX[usize::from(b >> 4)]));
            hex.push(char::from(HEX[usize::from(b & 0xf)]));
            hex
        });
    lines.push(format!("item\t{shown}\tfile\t{}\t{hash}", bytes.len()));
    Ok(())
}

/// `program` in `System32` (or under it), from `SystemRoot`.
fn system32(program: &str) -> PathBuf {
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
    PathBuf::from(root).join("System32").join(program)
}

/// Runs `program` with `args`, waits, and gives whether it succeeded and its stdout.
#[expect(
    clippy::disallowed_methods,
    reason = "the VM harness reads `reg query` and PowerShell's account lists, outside any agent"
)]
fn capture(program: &Path, args: &[&str]) -> Result<(bool, String), String> {
    let output = std::process::Command::new(program)
        .args(args)
        .output()
        .map_err(|e| format!("{}: {e}", program.display()))?;
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    Ok((output.status.success(), text))
}

/// The local users and groups, as listing lines (`user ...`, `group ...`).
pub(crate) fn accounts() -> Result<Vec<String>, String> {
    let script: Vec<u8> = ACCOUNTS.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let encoded = base64::engine::general_purpose::STANDARD.encode(script);
    let powershell = system32(r"WindowsPowerShell\v1.0\powershell.exe");
    let args = ["-NoProfile", "-NonInteractive", "-EncodedCommand", &encoded];
    let (ok, text) = capture(&powershell, &args)?;
    if !ok {
        return Err(format!("listing the accounts failed: {text}"));
    }
    let lines: Vec<String> = text
        .lines()
        .map(str::trim_end)
        .filter(|l| l.starts_with("user\t") || l.starts_with("group\t"))
        .map(str::to_owned)
        .collect();
    if !lines.iter().any(|l| l.starts_with("user\t")) {
        return Err(format!("no local user listed: {text}"));
    }
    Ok(lines)
}

/// The record: whether it is there (from the crate's read) and its values (`reg query`).
fn record() -> Result<Vec<String>, String> {
    let state = match sandbox_owner_sid() {
        Ok(Some(_)) => "present".to_owned(),
        Ok(None) => "absent".to_owned(),
        Err(e) => format!("unreadable ({e})"),
    };
    let mut lines = vec![format!("record\tstate\t{state}")];
    if state != "present" {
        return Ok(lines);
    }
    let key = format!(r"HKLM\{RECORD_KEY}");
    let (ok, text) = capture(&system32("reg.exe"), &["query", &key, "/s"])?;
    if !ok {
        return Err(format!("reg query {key} failed: {text}"));
    }
    for line in text.lines() {
        let words: Vec<&str> = line.split_whitespace().collect();
        if !words.is_empty() {
            lines.push(format!("recordvalue\t{}\t", words.join(" ")));
        }
    }
    Ok(lines)
}
