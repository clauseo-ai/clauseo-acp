//! The limits model through its public interface (`Limits`): the merge of reads and live
//! signals, the read policy, the deadlines, the view and `at_limit`. Time is a plain
//! `Timestamp` each test moves forward; nothing reads a clock.
//!
//! A test crate root resolves modules beside itself, so each submodule names its path. Each
//! carries `#[cfg(test)]` so clippy treats its helpers as test code.

#[cfg(test)]
#[path = "model/support.rs"]
mod support;

#[cfg(test)]
#[path = "model/merge.rs"]
mod merge;

#[cfg(test)]
#[path = "model/policy.rs"]
mod policy;

#[cfg(test)]
#[path = "model/deadlines.rs"]
mod deadlines;

#[cfg(test)]
#[path = "model/signed_out.rs"]
mod signed_out;

#[cfg(test)]
#[path = "model/view.rs"]
mod view;

#[cfg(test)]
#[path = "model/at_limit.rs"]
mod at_limit;
