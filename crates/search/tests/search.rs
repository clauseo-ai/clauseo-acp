//! The search index's tests, one binary: each module tests one part at the interface.
//!
//! A test crate root resolves modules beside itself, so each submodule in `search/` names its
//! path. Each carries `#[cfg(test)]` so clippy treats its helpers as test code.

#[cfg(test)]
#[path = "search/common.rs"]
mod common;
#[cfg(test)]
#[path = "search/corpus.rs"]
mod corpus;
#[cfg(test)]
#[path = "search/words.rs"]
mod words;
#[cfg(test)]
#[path = "search/write.rs"]
mod write;

#[cfg(test)]
#[path = "search/filters.rs"]
mod filters;
#[cfg(test)]
#[path = "search/hostile.rs"]
mod hostile;
#[cfg(test)]
#[path = "search/language.rs"]
mod language;
#[cfg(test)]
#[path = "search/normal.rs"]
mod normal;
#[cfg(test)]
#[path = "search/open.rs"]
mod open;
#[cfg(test)]
#[path = "search/paging.rs"]
mod paging;
#[cfg(test)]
#[path = "search/perf.rs"]
mod perf;
#[cfg(test)]
#[path = "search/rows.rs"]
mod rows;
