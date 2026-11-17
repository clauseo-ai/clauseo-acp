//! The file lines `CLAUSEO_REMOVE_OLD_PLUGINSDIRS` runs: `FindFirst` and `FindNext` on the
//! names in `Machine::found`, `Delete` (which fails on a path in `Machine::locked`) and
//! `RMDir /r`, each said. `FindClose` does nothing, in `Machine::line`. Also the template's
//! place-file lines: `File` and `FileOpen` (which fail while `Machine::held` counts down),
//! `FileClose`, `SetFileAttributes` and `Sleep`.

use super::Machine;

/// Runs `parts` when it is one of the file lines; false when it isn't, for `Machine::line`.
pub(super) fn file_line(machine: &mut Machine, parts: &[&str]) -> bool {
    match parts {
        // The pattern is said; the names come from `found`.
        ["FindFirst", handle, var, pattern] => {
            let pattern = machine.expand(pattern);
            machine.said.push(format!("FindFirst {pattern}"));
            machine.set(handle, "1".to_owned());
            find(machine, var);
        }
        ["FindNext", _, var] => find(machine, var),
        // A file in `locked` sets the error flag, as a file in use does.
        ["Delete", path] => {
            let path = machine.expand(path);
            machine.error |= machine.locked.contains(&path);
            machine.said.push(format!("Delete {path}"));
        }
        ["RMDir", "/r", path] => {
            let path = machine.expand(path);
            machine.said.push(format!("RMDir /r {path}"));
        }
        // Under SetOverwrite try a file NSIS can't open for writing is skipped with the error
        // flag set, and no box: each of the first `held` tries is one. Said, not expanded.
        ["File", ..] => {
            machine.said.push(parts.join(" "));
            if machine.held > 0 {
                machine.held -= 1;
                machine.error = true;
            }
        }
        // The error flag when the file isn't there, as NSIS sets it.
        ["SetFileAttributes", path, "NORMAL"] => {
            let path = machine.expand(path);
            machine.error |= !machine.files.contains(&path);
        }
        // The preflight's open for writing: fails while `held` counts down, as File does.
        ["FileOpen", var, path, "a"] => {
            let path = machine.expand(path);
            machine.said.push(format!("FileOpen {path}"));
            if machine.held > 0 {
                machine.held -= 1;
                machine.error = true;
                machine.set(var, String::new());
            } else {
                machine.set(var, "1".to_owned());
            }
        }
        ["FileClose", _] => {}
        ["Sleep", ms] => {
            let ms = machine.expand(ms);
            machine.said.push(format!("Sleep {ms}"));
        }
        _ => return false,
    }
    true
}

/// The next name `FindFirst` or `FindNext` returns into `var`; "" and the error flag at the
/// end, as NSIS leaves them.
fn find(machine: &mut Machine, var: &str) {
    if machine.found.is_empty() {
        machine.error = true;
        machine.set(var, String::new());
    } else {
        let name = machine.found.remove(0);
        machine.set(var, name);
    }
}
