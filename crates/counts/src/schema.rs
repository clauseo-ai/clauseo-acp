//! Schema 1 of the usage counts (research 37 §2, approved 2026-10-31): the day report, the
//! weekly tally and every value they may hold. No I/O and no clock; it builds for wasm32, so the
//! Worker validates with these same types (`default-features = false`).
//!
//! Every payload type is [`Closed`]: an enum, a capped integer, a bucket, a fixed-format day,
//! week or version, an id, or a struct or list of those. Every struct refuses a field it does not
//! know, every enum a name it does not know, every integer a value outside its cap. No string of
//! free text can be put in a payload, and none can be read out of one.

pub(crate) mod closed;
pub(crate) mod text;

mod buckets;
mod cells;
mod events;
mod ids;
mod numbers;
mod report;
mod time;
mod values;

#[cfg(test)]
pub(crate) mod tests;

pub use buckets::{Papers, StallTook, TimeOpenBucket, Took};
pub use cells::{ActiveWeek, Cell, DayCells, DayCounter, TallyCells, TallyCounter};
pub use closed::{Closed, require_closed, require_closed_value};
pub use events::{
    AgentInstall, AnswerEnded, AppOpened, BreakShown, Crash, ErrorShown, Event, FeatureUsed,
    FirstRead, Limit, MatterOpened, Onboarding, PermissionAsked, QuestionSent, SettingChanged,
    SignIn, Stall, TimeOpen, Update,
};
pub use ids::{
    BadText, InstallId, Platform, StrictErrorCode, TallyId, UUID_V4_PATTERN, VERSION_PATTERN,
    Version,
};
pub use numbers::{Count, One, OsMajor, OutOfRange, SchemaVersion, TextSize, Tries};
pub use report::{
    Batch, DayReport, Invalid, MAX_COUNTS_GZIP, MAX_COUNTS_JSON, MAX_DAY_AGE, MAX_DAYS_PER_BATCH,
    MAX_TALLY_AGE_WEEKS, MAX_TALLY_GZIP, MAX_TALLY_JSON, TallyReport, parse_batch, parse_tally,
};
pub use time::{BadDate, DAY_PATTERN, DAY_SECS, Day, Time, WEEK_PATTERN, Week};
pub use values::{
    Agent, AnswerOutcome, Arch, Billing, BreakKind, BreakThen, Channel, CrashKind, CrashProcess,
    Delivery, Feature, FirstReadResult, InstallResult, InstallType, Language, Launch, LimitKind,
    LiteMode, MatterKind, OnboardingStep, Os, PermissionAnswer, PermissionKind, Place, Protection,
    ReducedMotion, SignInResult, StallWhere, Theme, UpdateMode, UpdateResult,
};
