//! `probe items <plant|check|remove> <tag>`: the `end-cleanup` row's benign items, run by the
//! harness as the session's pool user (`win_logon.rs`), so each is that user's own: its HKCU
//! `Run` and `RunOnce` values, a `.cmd` in its Startup folder, a scheduled task registered as it
//! (a time trigger in 2099, never run) and a suspended BITS job with no files. Each is named
//! `ClauseoS15-<tag>` and runs (or would run) `"<this exe>" probe wait`.
//!
//! Prints one `item <kind> <word>` per kind, `kind` one of [`KINDS`]: `planted`, `present`,
//! `absent`, `removed`, or `error <why>`.

use std::path::PathBuf;
use std::process::{Command, ExitCode};

use super::cleanup_verdict::KINDS;
use super::sys::{make_dir, output, remove_file, say, write_file};

/// The HKCU key of the `Run` and `RunOnce` values.
const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion";

/// Runs `probe items <what> <tag>`.
pub(crate) fn run(rest: &[String]) -> ExitCode {
    let (Some(what), Some(tag)) = (rest.first(), rest.get(1)) else {
        say("usage: s15_launch_vm probe items <plant|check|remove> <tag>");
        return ExitCode::from(2);
    };
    let name = format!("ClauseoS15-{tag}");
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
            "check" => {
                present(kind, &name).map(|here| if here { "present" } else { "absent" }.to_owned())
            }
            "remove" => remove(kind, &name),
            _ => Err(format!("no such step {what:?}")),
        };
        say(&format!(
            "item {kind} {}",
            word.unwrap_or_else(|why| format!("error {why}"))
        ));
    }
    ExitCode::SUCCESS
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
        "bits" => exec_ok(
            "bitsadmin.exe",
            &["/rawreturn", "/create", "/download", name],
        ),
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
