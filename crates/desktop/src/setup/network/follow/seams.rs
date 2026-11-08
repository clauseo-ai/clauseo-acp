//! The test seams of [`Follower`]: a shorter budget, a fake probe, virtual time, a fake clock,
//! a shorter freshness.

use std::sync::Arc;
use std::time::Duration;

use clauseo_agents::Clock;

use super::super::LookupTime;
use super::Follower;

impl Follower {
    /// With `budget` instead of `OS_LOOKUP_BUDGET` (tests).
    pub(crate) fn lookup_budget(mut self, budget: Duration) -> Self {
        self.resolver.budget = budget;
        self
    }

    /// With `reach` instead of a real probe of a proxy (tests).
    pub(crate) fn reach(mut self, reach: impl Fn(&str) -> bool + Send + Sync + 'static) -> Self {
        self.resolver.reach = Arc::new(reach);
        self
    }

    /// With `time` for the lookups' budget and the probes instead of `RealTime` (tests).
    pub(crate) fn lookup_time(mut self, time: Arc<dyn LookupTime>) -> Self {
        self.resolver.time = time;
        self
    }

    /// With `clock` instead of the system's (tests).
    pub(crate) fn clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.fresh.clock = clock;
        self
    }

    /// With `fresh_for` instead of `ROUTE_FRESH_FOR` (tests).
    pub(crate) fn fresh_for(mut self, fresh_for: Duration) -> Self {
        self.fresh.fresh_for = fresh_for;
        self
    }
}
