//! The grants: the Claude store and the browser socket folder, which the jail allows again, and
//! the rules every folder it allows again keeps ([`check_not_holding`], [`check_outside_matter`]).

use std::path::{Path, PathBuf};

use super::build::{existing, overlap, refuse};
use super::{ClaudeStore, under};
use crate::Error;

/// A folder the jail allows again that holds the agent's writes (`what`, with its article), on a
/// canonical `p`: it may not equal or hold the home folder, the state folder or a hidden path.
/// The temp folder and the grants ([`check_grant`]) keep this rule.
pub(super) fn check_not_holding(
    p: &Path,
    what: &str,
    home: Option<&Path>,
    state: &Path,
    hidden: &[PathBuf],
) -> Result<(), Error> {
    let shown = p.display();
    if home.is_some_and(|home| under(home, p)) {
        return refuse(format!(
            "the {what} may not equal or hold the home folder: {shown}"
        ));
    }
    if under(state, p) {
        return refuse(format!(
            "the {what} may not equal or hold the state folder: {shown} and {}",
            state.display()
        ));
    }
    if let Some(h) = hidden.iter().find(|h| under(h, p)) {
        return refuse(format!(
            "the {what} may not equal or hold a hidden path: {shown} and {}",
            h.display()
        ));
    }
    Ok(())
}

/// A folder the jail allows again that is not inside the matter nor holds it (`what`, with its
/// article), on a canonical `p`: its writes would reach the matter. The notes folder, the
/// temp folder and the grants ([`check_grant`]) all keep this rule.
pub(super) fn check_outside_matter(p: &Path, what: &str, matter: &Path) -> Result<(), Error> {
    if overlap(p, matter) {
        return refuse(format!(
            "the {what} may not be inside the matter or hold it: {} and {}",
            p.display(),
            matter.display()
        ));
    }
    Ok(())
}

/// Claude's login store folder, if given: canonical by [`existing`], and held to [`check_grant`].
pub(super) fn claude_store_folder(
    p: Option<&Path>,
    places: &Places<'_>,
) -> Result<Option<ClaudeStore>, Error> {
    let Some(p) = p else {
        return Ok(None);
    };
    let store = ClaudeStore {
        dir: existing(p, "Claude store folder")?,
    };
    check_grant(store.dir(), "Claude store", places)?;
    Ok(Some(store))
}

/// The browser socket folder, if given, held to [`check_grant`]: the jail allows Unix sockets and
/// writes under it.
pub(super) fn check_socket_folder(socket: Option<&Path>, places: &Places<'_>) -> Result<(), Error> {
    socket.map_or(Ok(()), |socket| {
        check_grant(socket, "browser socket folder", places)
    })
}

/// The places a folder the jail allows again (a grant, `what`, with its article) may not sit
/// inside or hold: the matter, and the state folder, the home folder and the hidden paths.
pub(super) struct Places<'a> {
    matter: &'a Path,
    state: &'a Path,
    home: Option<&'a Path>,
    hidden: &'a [PathBuf],
}

impl<'a> Places<'a> {
    pub(super) fn new(
        matter: &'a Path,
        state: &'a Path,
        home: Option<&'a Path>,
        hidden: &'a [PathBuf],
    ) -> Self {
        Places {
            matter,
            state,
            home,
            hidden,
        }
    }
}

/// A grant's rules, on a canonical `p`: it may not be inside the matter or hold it, nor equal or
/// hold the home folder, the state folder or a hidden path ([`check_not_holding`]).
fn check_grant(p: &Path, what: &str, places: &Places<'_>) -> Result<(), Error> {
    check_outside_matter(p, what, places.matter)?;
    check_not_holding(p, what, places.home, places.state, places.hidden)
}
