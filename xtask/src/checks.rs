//! Static checks: Rust code that inspects the repo instead of running a tool.
//!
//! One file per check in `src/checks/`, registered in `ALL` below. `steps::plan` runs every entry
//! after the tool steps and before nextest.

mod budgets;

use crate::violation::Violation;
use crate::workspace::Context;

/// A check written in Rust. It fails iff it returns violations.
pub(crate) struct StaticCheck {
    pub name: &'static str,
    pub run: fn(&Context) -> Vec<Violation>,
}

/// Every static check, in run order.
pub(crate) const ALL: &[StaticCheck] = &[budgets::CHECK];
