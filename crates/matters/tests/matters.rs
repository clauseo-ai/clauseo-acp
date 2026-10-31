//! The matters crate through its public interface, in real folders.
//!
//! A test crate root resolves modules beside itself, so each submodule in `matters/` names its
//! path. Each carries `#[cfg(test)]` so clippy treats its helpers as test code.

#[cfg(test)]
#[path = "matters/support.rs"]
mod support;

#[cfg(test)]
#[path = "matters/fixture.rs"]
mod fixture;

#[cfg(test)]
#[path = "matters/scan.rs"]
mod scan;

#[cfg(test)]
#[path = "matters/create.rs"]
mod create;

#[cfg(test)]
#[path = "matters/checkpoint.rs"]
mod checkpoint;

#[cfg(test)]
#[path = "matters/skipped.rs"]
mod skipped;

#[cfg(test)]
#[path = "matters/undo.rs"]
mod undo;

#[cfg(test)]
#[path = "matters/undo_case.rs"]
mod undo_case;

#[cfg(test)]
#[path = "matters/restore.rs"]
mod restore;

#[cfg(test)]
#[path = "matters/compare.rs"]
mod compare;

#[cfg(test)]
#[path = "matters/reopen.rs"]
mod reopen;

#[cfg(test)]
#[path = "matters/owner_file.rs"]
mod owner_file;

#[cfg(test)]
#[path = "matters/locate.rs"]
mod locate;

#[cfg(test)]
#[path = "matters/errors.rs"]
mod errors;

#[cfg(test)]
#[path = "matters/refuse.rs"]
mod refuse;
