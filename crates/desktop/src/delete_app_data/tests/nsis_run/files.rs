//! The file lines `CLAUSEO_REMOVE_OLD_PLUGINSDIRS` runs: `FindFirst` and `FindNext` on the
//! names in `Machine::found`, `Delete` (which fails on a path in `Machine::locked`) and
//! `RMDir /r`, each said. `FindClose` does nothing, in `Machine::line`.

use super::Machine;

impl Machine {
    /// Runs `parts` when it is one of the file lines; false when it isn't, for `Machine::line`.
    pub(super) fn file_line(&mut self, parts: &[&str]) -> bool {
        match parts {
            // The pattern is said; the names come from `found`.
            ["FindFirst", handle, var, pattern] => {
                let pattern = self.expand(pattern);
                self.said.push(format!("FindFirst {pattern}"));
                self.set(handle, "1".to_owned());
                self.find(var);
            }
            ["FindNext", _, var] => self.find(var),
            // A file in `locked` sets the error flag, as a file in use does.
            ["Delete", path] => {
                let path = self.expand(path);
                self.error |= self.locked.contains(&path);
                self.said.push(format!("Delete {path}"));
            }
            ["RMDir", "/r", path] => {
                let path = self.expand(path);
                self.said.push(format!("RMDir /r {path}"));
            }
            _ => return false,
        }
        true
    }

    /// The next name `FindFirst` or `FindNext` returns into `var`; "" and the error flag at the
    /// end, as NSIS leaves them.
    fn find(&mut self, var: &str) {
        if self.found.is_empty() {
            self.error = true;
            self.set(var, String::new());
        } else {
            let name = self.found.remove(0);
            self.set(var, name);
        }
    }
}
