//! `probe items <plant|check|remove> <tag>`: the `end-cleanup` row's benign items, run by the
//! harness as the session's pool user (`win_logon.rs`), so each is that user's own: its HKCU
//! `Run` and `RunOnce` values, a `.cmd` in its Startup folder, a scheduled task registered as it
//! (a time trigger in 2099, never run), each running `"<this exe>" probe wait`, and a BITS job
//! that could complete. Each is named `ClauseoS15-<tag>`.
//!
//! The BITS job copies a local file to another (`file://` source), both in the folder
//! `%TEMP%\ClauseoS15-<tag>`, is resumed, and has a notify command line,
//! `"<this exe>" probe items mark <tag> <marker>`, which BITS runs once the job is transferred and
//! which writes the marker file `fired` in that folder. The marker proves the job ran: BITS holds
//! no logon token for a pool user, so it shouldn't (`cleanup_verdict.rs`).
//!
//! Prints one `item <kind> <word>` per kind, `kind` one of [`KINDS`]: `planted`, `present`,
//! `absent`, `removed`, or `error <why>`; the BITS job's `present` is followed by its state
//! (`bitsadmin /getstate`). A check also prints `marker bits <present|absent>`; a removal
//! removes the folder too.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use super::cleanup_verdict::KINDS;
use super::sys::{make_dir, output, remove_file, remove_tree, say, write_file};

/// The marker file's name, in the BITS job's folder.
const MARKER: &str = "fired";

/// The HKCU key of the `Run` and `RunOnce` values.
const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion";

/// Runs `probe items <what> <tag>`.
pub(crate) fn run(rest: &[String]) -> ExitCode {
    let (Some(what), Some(tag)) = (rest.first(), rest.get(1)) else {
        say("usage: s15_launch_vm probe items <plant|check|remove> <tag>");
        return ExitCode::from(2);
    };
    let name = format!("ClauseoS15-{tag}");
    if what == "mark" {
        return mark(&name, rest.get(2));
    }
    let exe = match std::env::current_exe() {
        Ok(exe) => exe.display().to_string(),
        Err(e) => {
            say(&format!("items error current_exe: {e}"));
            return ExitCode::from(1);
        }
    };
    for kind in KINDS {
        let word = match what.as_str() {
            "plant" => plant(kind, &name, &exe).map(|()| "planted".to_owned()),
            "check" => present(kind, &name).map(|here| match here {
                false => "absent".to_owned(),
                true if kind == "bits" => format!("present {}", bits_state(&name)),
                true => "present".to_owned(),
            }),
            "remove" => remove(kind, &name),
            _ => Err(format!("no such step {what:?}")),
        };
        say(&format!(
            "item {kind} {}",
            word.unwrap_or_else(|why| format!("error {why}"))
        ));
    }
    match (what.as_str(), bits_dir(&name)) {
        ("check", Ok(dir)) => say(&format!(
            "marker bits {}",
            if dir.join(MARKER).exists() {
                "present"
            } else {
                "absent"
            }
        )),
        ("remove", Ok(dir)) if dir.exists() => {
            if let Err(e) = remove_tree(&dir) {
                say(&format!("folder bits error {}: {e}", dir.display()));
            }
        }
        (_, Err(why)) => say(&format!("marker bits error {why}")),
        _ => {}
    }
    ExitCode::SUCCESS
}

/// `probe items mark <tag> <marker>`, the BITS job's notify command: writes `marker`, which
/// must be the marker file of the job's folder.
fn mark(name: &str, marker: Option<&String>) -> ExitCode {
    let ours = bits_dir(name).map(|dir| dir.join(MARKER));
    match (marker, ours) {
        (Some(marker), Ok(ours)) if Path::new(marker) == ours => {
            match write_file(&ours, b"fired\r\n") {
                Ok(()) => ExitCode::SUCCESS,
                Err(_) => ExitCode::from(1),
            }
        }
        _ => ExitCode::from(2),
    }
}

/// `%TEMP%\<name>`: the BITS job's source, destination and marker.
fn bits_dir(name: &str) -> Result<PathBuf, String> {
    let temp = std::env::var("TEMP").map_err(|e| format!("TEMP: {e}"))?;
    Ok(PathBuf::from(temp).join(name))
}

/// `path` as a `file:///` URL.
fn file_url(path: &Path) -> String {
    format!("file:///{}", path.display().to_string().replace('\\', "/"))
}

/// The job's state as `bitsadmin /getstate` gives it, or `unread`.
fn bits_state(name: &str) -> String {
    match exec("bitsadmin.exe", &["/rawreturn", "/getstate", name]) {
        Ok((0, text)) if !text.trim().is_empty() => text.trim().to_owned(),
        _ => "unread".to_owned(),
    }
}

/// Plants the BITS job: made, a local file to copy, the notify command, resumed.
fn plant_bits(name: &str, exe: &str) -> Result<(), String> {
    let dir = bits_dir(name)?;
    make_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let source = dir.join("source.txt");
    write_file(&source, b"ClauseoS15\r\n").map_err(|e| format!("{}: {e}", source.display()))?;
    let destination = dir.join("copy.txt").display().to_string();
    let marker = dir.join(MARKER).display().to_string();
    let notify = format!("\"{exe}\" probe items mark {} \"{marker}\"", tag_of(name));
    exec_ok(
        "bitsadmin.exe",
        &["/rawreturn", "/create", "/download", name],
    )?;
    let url = file_url(&source);
    exec_ok(
        "bitsadmin.exe",
        &["/rawreturn", "/addfile", name, &url, &destination],
    )?;
    exec_ok(
        "bitsadmin.exe",
        &["/rawreturn", "/setnotifyflags", name, "3"],
    )?;
    exec_ok(
        "bitsadmin.exe",
        &["/rawreturn", "/setnotifycmdline", name, exe, &notify],
    )?;
    exec_ok("bitsadmin.exe", &["/rawreturn", "/resume", name])
}

/// The tag in `ClauseoS15-<tag>`.
fn tag_of(name: &str) -> &str {
    name.strip_prefix("ClauseoS15-").unwrap_or(name)
}

/// Runs `program args` to its end; its exit code and its stdout, or why it couldn't start.
fn exec(program: &str, args: &[&str]) -> Result<(i32, String), String> {
    let done = output(Command::new(program).args(args)).map_err(|e| format!("{program}: {e}"))?;
    let text = String::from_utf8_lossy(&done.stdout).into_owned();
    Ok((done.status.code().unwrap_or(-1), text))
}

/// `program args`, which must exit 0.
fn exec_ok(program: &str, args: &[&str]) -> Result<(), String> {
    match exec(program, args)? {
        (0, _) => Ok(()),
        (code, text) => Err(format!("{program} exited {code}: {}", text.trim())),
    }
}

/// `<APPDATA>\Microsoft\Windows\Start Menu\Programs\Startup\<name>.cmd`.
fn startup_file(name: &str) -> Result<PathBuf, String> {
    let appdata = std::env::var("APPDATA").map_err(|e| format!("APPDATA: {e}"))?;
    Ok(PathBuf::from(appdata)
        .join(r"Microsoft\Windows\Start Menu\Programs\Startup")
        .join(format!("{name}.cmd")))
}

/// `text` with XML's special characters escaped.
fn xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// The scheduled task's definition: this user, its interactive token only (no password kept),
/// a time trigger in 2099, the probe's `wait`.
fn task_xml(exe: &str) -> String {
    let user = match (std::env::var("USERDOMAIN"), std::env::var("USERNAME")) {
        (Ok(domain), Ok(name)) => format!("<UserId>{}</UserId>", xml(&format!(r"{domain}\{name}"))),
        _ => String::new(),
    };
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-16\"?>\r\n\
         <Task version=\"1.2\" xmlns=\"http://schemas.microsoft.com/windows/2004/02/mit/task\">\
         <Triggers><TimeTrigger><StartBoundary>2099-01-01T00:00:00</StartBoundary>\
         <Enabled>true</Enabled></TimeTrigger></Triggers>\
         <Principals><Principal id=\"Author\">{user}<LogonType>InteractiveToken</LogonType>\
         <RunLevel>LeastPrivilege</RunLevel></Principal></Principals>\
         <Settings><Enabled>true</Enabled></Settings>\
         <Actions Context=\"Author\"><Exec><Command>{}</Command><Arguments>probe wait\
         </Arguments></Exec></Actions></Task>",
        xml(exe)
    )
}

fn plant(kind: &str, name: &str, exe: &str) -> Result<(), String> {
    let line = format!("\"{exe}\" probe wait");
    match kind {
        "run" | "runonce" => {
            let key = format!(
                r"{RUN_KEY}\{}",
                if kind == "run" { "Run" } else { "RunOnce" }
            );
            exec_ok(
                "reg.exe",
                &["add", &key, "/v", name, "/t", "REG_SZ", "/d", &line, "/f"],
            )
        }
        "startup" => {
            let file = startup_file(name)?;
            if let Some(folder) = file.parent() {
                make_dir(folder).map_err(|e| format!("{}: {e}", folder.display()))?;
            }
            write_file(&file, format!("@{line}\r\n").as_bytes())
                .map_err(|e| format!("{}: {e}", file.display()))
        }
        "task" => {
            let temp = std::env::temp_dir().join(format!("{name}.xml"));
            let mut bytes = vec![0xFF, 0xFE];
            bytes.extend(task_xml(exe).encode_utf16().flat_map(u16::to_le_bytes));
            write_file(&temp, &bytes).map_err(|e| format!("{}: {e}", temp.display()))?;
            let xml_path = temp.display().to_string();
            let made = exec_ok(
                "schtasks.exe",
                &["/create", "/tn", name, "/xml", &xml_path, "/f"],
            );
            drop(remove_file(&temp));
            made
        }
        "bits" => plant_bits(name, exe),
        _ => Err(format!("no kind {kind}")),
    }
}

fn present(kind: &str, name: &str) -> Result<bool, String> {
    match kind {
        "run" | "runonce" => {
            let key = format!(
                r"{RUN_KEY}\{}",
                if kind == "run" { "Run" } else { "RunOnce" }
            );
            Ok(exec("reg.exe", &["query", &key, "/v", name])?.0 == 0)
        }
        "startup" => Ok(startup_file(name)?.exists()),
        "task" => Ok(exec("schtasks.exe", &["/query", "/tn", name])?.0 == 0),
        "bits" => match exec("bitsadmin.exe", &["/list"])? {
            (0, text) => Ok(text.contains(&format!("'{name}'"))),
            (code, text) => Err(format!("bitsadmin /list exited {code}: {}", text.trim())),
        },
        _ => Err(format!("no kind {kind}")),
    }
}

/// Removes the item when it is there: `removed`, or `absent` when it wasn't.
fn remove(kind: &str, name: &str) -> Result<String, String> {
    if !present(kind, name)? {
        return Ok("absent".to_owned());
    }
    match kind {
        "run" | "runonce" => {
            let key = format!(
                r"{RUN_KEY}\{}",
                if kind == "run" { "Run" } else { "RunOnce" }
            );
            exec_ok("reg.exe", &["delete", &key, "/v", name, "/f"])?;
        }
        "startup" => {
            let file = startup_file(name)?;
            remove_file(&file).map_err(|e| format!("{}: {e}", file.display()))?;
        }
        "task" => exec_ok("schtasks.exe", &["/delete", "/tn", name, "/f"])?,
        "bits" => exec_ok("bitsadmin.exe", &["/rawreturn", "/cancel", name])?,
        _ => return Err(format!("no kind {kind}")),
    }
    Ok("removed".to_owned())
}
