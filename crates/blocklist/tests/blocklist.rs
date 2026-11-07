//! The block list's tests: parse, verify, matching, the rollback rule and the shared list.
//! Fixtures are in `tests/data` (`make_fixtures.py` writes them, with throwaway keys).

#![forbid(clippy::disallowed_methods, clippy::disallowed_types)]

#[cfg(test)]
#[path = "blocklist/current.rs"]
mod current;
#[cfg(test)]
#[path = "blocklist/fixtures.rs"]
mod fixtures;
#[cfg(test)]
#[path = "blocklist/matching.rs"]
mod matching;
#[cfg(test)]
#[path = "blocklist/parse.rs"]
mod parse;
#[cfg(test)]
#[path = "blocklist/rollback.rs"]
mod rollback;
#[cfg(test)]
#[path = "blocklist/verify.rs"]
mod verify;
