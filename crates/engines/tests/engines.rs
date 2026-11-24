//! The engine list's tests: the Tauri signature, the shape, the official names, the accept
//! rules and the compiled-in key. Fixtures are in `tests/data` (`make_fixtures.mjs` writes
//! them with `tauri signer sign` and throwaway keys whose private halves were never committed).

#![forbid(clippy::disallowed_methods, clippy::disallowed_types)]

#[cfg(test)]
#[path = "engines/accept.rs"]
mod accept;
#[cfg(test)]
#[path = "engines/fixtures.rs"]
mod fixtures;
#[cfg(test)]
#[path = "engines/key.rs"]
mod key;
#[cfg(test)]
#[path = "engines/lock.rs"]
mod lock;
#[cfg(test)]
#[path = "engines/names.rs"]
mod names;
#[cfg(test)]
#[path = "engines/shape.rs"]
mod shape;
#[cfg(test)]
#[path = "engines/verify.rs"]
mod verify;
