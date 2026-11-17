//! The matter's version history as the agents read it with the computer's own `git`: the
//! `history.git` of `clauseo_matters` (`<data>/matters/<matter id>/history.git`, decision record
//! 0007, `matter_worker::matter_settings`), named in the instructions (`{matter_history}`) and
//! opened for reading in Claude's sandbox (`loads::sandbox`), its Windows shell rules
//! (`launch_checks::folders`) and the host's auto-deny (`actor::turn::asks`). Never writable:
//! the stat cache is its `index`. Found from the matter's notes folder,
//! `<data>/agent-state/<matter id>/notes`, which every launch already carries.

use std::path::{Path, PathBuf};

/// This matter's `history.git` for its notes folder `notes`
/// (`<data>/agent-state/<matter id>/notes`); `None` for a path not of that shape.
pub(crate) fn matter_history(notes: &Path) -> Option<PathBuf> {
    let state = notes.parent()?;
    let matter = state.file_name()?;
    let data = state
        .parent()
        .filter(|dir| dir.ends_with("agent-state"))?
        .parent()?;
    Some(data.join("matters").join(matter).join("history.git"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_history_sits_in_the_matters_root_beside_the_notes() {
        let data = Path::new("/d/ai.clauseo.desktop");
        let matter = "0190f5c2-0000-7000-8000-000000000007";
        let notes = data.join("agent-state").join(matter).join("notes");
        let root = crate::matter_worker::matter_settings(data).matters_root;
        let want = root.join(matter).join("history.git");
        assert_eq!(matter_history(&notes), Some(want));
        assert_eq!(matter_history(Path::new("/d/x/notes")), None);
    }
}
