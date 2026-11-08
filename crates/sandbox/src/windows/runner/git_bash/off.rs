//! The Git Bash stub a run with Git Bash off gets ([`off_stub`]). Claude Code, given no
//! `CLAUDE_CODE_GIT_BASH_PATH` or one naming no file, looks for Git Bash itself and finds the
//! lawyer's (2.1.317: `C:\Program Files\Git\bin\bash.exe`, the `(x86)` one, then `git` on
//! `PATH`). So the variable always names a stub, in this order:
//!
//! 1. the machine's: `<admin>\git\_off\bash.exe`, which set-up and the SYSTEM refresh place.
//!    `git` and `_off` must pass `git_tree::checked_chain` (opened as themselves, set-up's
//!    owner and exact DACL), and the file, opened as itself relative to `_off`,
//!    `admin_copy::level_safe` (owner SYSTEM, Administrators or `TrustedInstaller`; no one else
//!    may change it);
//! 2. the outer runner's own: its exe copied as `<data_dir>\sandbox\git-off\bash.exe`. The
//!    data folder must lie in `LocalAppData` (the known folder, never the environment; never
//!    roaming) on a local drive, never a network one. `git-off` is the desktop user's, with only
//!    its owner and the sandbox users' group in its protected DACL, the group to read and run
//!    ([`own_dir_sddl`]); the pool user reaches it by bypassing traverse checks, as it does the
//!    trees. The copy is hashed
//!    at each launch and copied again whenever its SHA-256 isn't the runner's, then hashed
//!    again before the variable names it. It runs as the pool user, as the runner it copies
//!    does, so nothing gains a right;
//! 3. neither: the launch is refused ([`NO_STUB`]), so the agent never runs where Claude Code
//!    could find the lawyer's Git Bash.

use super::super::paths::WinPath;
use super::GitBashOs;
use crate::windows::admin_copy::level_safe;
use crate::windows::git_tree::{GIT_DIR, OFF_DIR, STUB_NAME, git_dir_sddl, off_sddl};
use crate::windows::sids::Sid;

/// The outer runner's own stub folder, from the data folder.
pub(crate) const OWN_DIR: [&str; 2] = ["sandbox", "git-off"];
/// The plain line of a launch refused for want of a stub.
pub(crate) const NO_STUB: &str = "Git Bash couldn't be turned off safely, so the agent won't start";

/// The stub a run with Git Bash off gets, with why the machine's was passed over when the
/// outer runner's own is used; else why neither can be, for [`NO_STUB`]'s line.
pub(crate) fn off_stub<O: GitBashOs>(
    os: &mut O,
    desktop: &Sid,
    data_dir: &str,
) -> Result<(WinPath, Option<String>), String> {
    let machine = match machine_stub(os) {
        Ok(stub) => return Ok((stub, None)),
        Err(why) => why,
    };
    match own_stub(os, desktop, data_dir) {
        Ok(stub) => Ok((stub, Some(machine))),
        Err(own) => Err(format!(
            "the machine's stub: {machine}; the runner's own: {own}"
        )),
    }
}

/// The machine's stub, once its folders and the file pass their checks (step 1).
pub(crate) fn machine_stub<O: GitBashOs>(os: &mut O) -> Result<WinPath, String> {
    let admin = os.admin_dir()?;
    let group = os.group()?;
    let levels = [
        (GIT_DIR.to_owned(), git_dir_sddl(&group)),
        (OFF_DIR.to_owned(), off_sddl(&group)),
    ];
    let security = os.stub_security(&admin, &levels, STUB_NAME)?;
    if !level_safe(&security) {
        return Err("its owner or DACL lets another account change it".to_owned());
    }
    Ok(admin.join(GIT_DIR).join(OFF_DIR).join(STUB_NAME))
}

/// The outer runner's own stub, made, checked and copied again as needed (step 2).
pub(crate) fn own_stub<O: GitBashOs>(
    os: &mut O,
    desktop: &Sid,
    data_dir: &str,
) -> Result<WinPath, String> {
    let local = |text: &str| WinPath::parse(text).filter(|path| path.drive().is_some());
    let base = local(&os.local_app_data()?)
        .ok_or_else(|| "LocalAppData isn't on a local drive".to_owned())?;
    let data = local(data_dir)
        .filter(|data| data.within(&base))
        .ok_or_else(|| "the data folder isn't in LocalAppData".to_owned())?;
    let dir = OWN_DIR.iter().fold(data, |path, part| path.join(part));
    let group = os.group()?;
    os.secure_dir(&dir, &own_dir_sddl(desktop, &group))?;
    let stub = dir.join(STUB_NAME);
    let runner = os.runner_hash()?;
    if os.file_hash(&stub)?.as_ref() != Some(&runner) {
        os.copy_runner(&stub)?;
        if os.file_hash(&stub)? != Some(runner) {
            return Err("the copy doesn't match the runner".to_owned());
        }
    }
    Ok(stub)
}

/// The outer runner's own stub folder's descriptor: owner the desktop user, full; the sandbox
/// users' group read and execute (`0x1200a9`); both inherited by the copy; nothing else.
pub(crate) fn own_dir_sddl(desktop: &Sid, group: &Sid) -> String {
    format!("O:{desktop}D:P(A;OICI;FA;;;{desktop})(A;OICI;0x1200a9;;;{group})")
}
