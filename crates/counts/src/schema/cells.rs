//! Cells: one per combination of an event's fields, each with its count. [`DayCells`] and
//! [`TallyCells`] are how they travel (`"cells": { "<event>": [ { "<field>": …, "n": … } ] }`,
//! with only non-zero cells); [`DayCounter`] and [`TallyCounter`] add them up in memory.

use std::collections::{BTreeMap, BTreeSet};

use serde::de::Error as _;
use serde::ser::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::closed::{seal, sealed};
use super::events::{
    AgentInstall, AnswerEnded, AppOpened, BreakShown, Crash, ErrorShown, Event, FeatureUsed,
    FirstRead, Limit, MatterOpened, Onboarding, PermissionAsked, QuestionSent, SettingChanged,
    SignIn, Stall, TimeOpen, Update,
};
use super::{Closed, Count, One, TimeOpenBucket};

/// One cell: a combination of an event's field values, and how many times it happened. On the
/// wire the fields and `n` sit side by side in one object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell<K> {
    /// The field values.
    pub key: K,
    /// How many times, 1 to 9,999.
    pub n: Count,
}

impl<K: Closed> sealed::Sealed for Cell<K> {}
impl<K: Closed> Closed for Cell<K> {}

impl<K: Serialize> Serialize for Cell<K> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut value = serde_json::to_value(&self.key).map_err(S::Error::custom)?;
        let serde_json::Value::Object(fields) = &mut value else {
            return Err(S::Error::custom("a cell's fields are an object"));
        };
        fields.insert("n".to_owned(), self.n.get().into());
        value.serialize(serializer)
    }
}

impl<'de, K: serde::de::DeserializeOwned> Deserialize<'de> for Cell<K> {
    /// The object less `n` must be exactly `K`, which refuses any field it does not know.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let serde_json::Value::Object(mut fields) = serde_json::Value::deserialize(deserializer)?
        else {
            return Err(D::Error::custom("a cell is an object"));
        };
        let n = fields
            .remove("n")
            .ok_or_else(|| D::Error::missing_field("n"))?;
        let n = Count::deserialize(n).map_err(D::Error::custom)?;
        let key = K::deserialize(serde_json::Value::Object(fields)).map_err(D::Error::custom)?;
        Ok(Cell { key, n })
    }
}

/// Adds one to `key`'s cell, which stops at 9,999.
fn add<K: Ord>(cells: &mut BTreeMap<K, Count>, key: K) {
    add_n(cells, key, Count::ONE);
}

fn add_n<K: Ord>(cells: &mut BTreeMap<K, Count>, key: K, n: Count) {
    cells
        .entry(key)
        .and_modify(|count| *count = count.plus(n))
        .or_insert(n);
}

fn to_cells<K: Copy>(cells: &BTreeMap<K, Count>) -> Vec<Cell<K>> {
    cells.iter().map(|(&key, &n)| Cell { key, n }).collect()
}

fn has_duplicates<K: Ord>(cells: &[Cell<K>]) -> bool {
    let mut seen = BTreeSet::new();
    cells.iter().any(|cell| !seen.insert(&cell.key))
}

/// The day report's cells, and the counter that adds them up, from one list of events.
macro_rules! day_cells {
    ($($field:ident: $key:ty,)+) => {
        /// The day report's cells: one list per event, left out when empty.
        #[derive(
            Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize,
        )]
        #[serde(deny_unknown_fields)]
        #[cfg_attr(test, derive(schemars::JsonSchema))]
        pub struct DayCells {
            $(
                #[doc = concat!("`", stringify!($field), "`.")]
                #[serde(default, skip_serializing_if = "Vec::is_empty")]
                pub $field: Vec<Cell<$key>>,
            )+
        }

        seal!(DayCells { $($field),+ });

        /// One day's cells as they are added up.
        #[derive(Debug, Clone, Default, PartialEq, Eq)]
        pub struct DayCounter {
            $($field: BTreeMap<$key, Count>,)+
        }

        impl DayCounter {
            /// Whether no cell holds a count.
            pub fn is_empty(&self) -> bool {
                true $(&& self.$field.is_empty())+
            }

            /// The cells to send or save.
            pub fn to_cells(&self) -> DayCells {
                DayCells { $($field: to_cells(&self.$field),)+ }
            }

            /// Adds saved cells in, cell by cell.
            pub fn merge(&mut self, cells: &DayCells) {
                $(for cell in &cells.$field {
                    add_n(&mut self.$field, cell.key, cell.n);
                })+
            }
        }

        impl DayCells {
            /// Whether two cells of one event hold the same combination, or `time_open` holds
            /// more than its one cell.
            pub fn is_malformed(&self) -> bool {
                self.time_open.len() > 1 $(|| has_duplicates(&self.$field))+
            }

            /// Whether no event has a cell.
            pub fn is_empty(&self) -> bool {
                true $(&& self.$field.is_empty())+
            }
        }
    };
}

day_cells! {
    app_opened: AppOpened,
    time_open: TimeOpen,
    onboarding_step: Onboarding,
    agent_install: AgentInstall,
    sign_in: SignIn,
    matter_opened: MatterOpened,
    first_read: FirstRead,
    question_sent: QuestionSent,
    answer_ended: AnswerEnded,
    feature_used: FeatureUsed,
    permission_asked: PermissionAsked,
    limit: Limit,
    break_shown: BreakShown,
    error_shown: ErrorShown,
    update: Update,
    setting_changed: SettingChanged,
    stall: Stall,
}

impl DayCounter {
    /// Counts `event` once. `crash` belongs to the tally, so it is not counted here.
    pub fn bump(&mut self, event: Event) {
        match event {
            Event::AppOpened(key) => add(&mut self.app_opened, key),
            Event::Onboarding(key) => add(&mut self.onboarding_step, key),
            Event::AgentInstall(key) => add(&mut self.agent_install, key),
            Event::SignIn(key) => add(&mut self.sign_in, key),
            Event::MatterOpened(key) => add(&mut self.matter_opened, key),
            Event::FirstRead(key) => add(&mut self.first_read, key),
            Event::QuestionSent(key) => add(&mut self.question_sent, key),
            Event::AnswerEnded(key) => add(&mut self.answer_ended, key),
            Event::FeatureUsed(key) => add(&mut self.feature_used, key),
            Event::PermissionAsked(key) => add(&mut self.permission_asked, key),
            Event::Limit(key) => add(&mut self.limit, key),
            Event::BreakShown(key) => add(&mut self.break_shown, key),
            Event::ErrorShown(key) => add(&mut self.error_shown, key),
            Event::Update(key) => add(&mut self.update, key),
            Event::SettingChanged(key) => add(&mut self.setting_changed, key),
            Event::Stall(key) => add(&mut self.stall, key),
            Event::Crash(_) => {}
        }
    }

    /// Sets the day's one `time_open` cell from its foreground seconds (none for zero).
    pub fn set_time_open(&mut self, foreground_secs: u64) {
        self.time_open.clear();
        if foreground_secs > 0 {
            let bucket = TimeOpenBucket::from_secs(foreground_secs);
            self.time_open.insert(TimeOpen { bucket }, Count::ONE);
        }
    }
}

/// An `active_week` cell: the install ran that week. Its count is always 1.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
pub struct ActiveWeek {
    /// 1.
    pub n: One,
}

seal!(ActiveWeek { n });

/// The weekly tally's cells.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
pub struct TallyCells {
    /// `crash`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub crash: Vec<Cell<Crash>>,
    /// `active_week`: one cell, or none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub active_week: Vec<ActiveWeek>,
}

seal!(TallyCells { crash, active_week });

impl TallyCells {
    /// Whether two crash cells hold the same combination, or `active_week` holds more than one.
    pub fn is_malformed(&self) -> bool {
        self.active_week.len() > 1 || has_duplicates(&self.crash)
    }
}

/// One week's tally cells as they are added up.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TallyCounter {
    crash: BTreeMap<Crash, Count>,
    active: bool,
}

impl TallyCounter {
    /// Counts a crash once.
    pub fn crash(&mut self, crash: Crash) {
        add(&mut self.crash, crash);
    }

    /// Marks the week active; returns whether it was not yet.
    pub fn mark_active(&mut self) -> bool {
        !std::mem::replace(&mut self.active, true)
    }

    /// The cells to send or save.
    pub fn to_cells(&self) -> TallyCells {
        TallyCells {
            crash: to_cells(&self.crash),
            active_week: if self.active {
                vec![ActiveWeek { n: One::ONE }]
            } else {
                Vec::new()
            },
        }
    }

    /// Adds saved cells in.
    pub fn merge(&mut self, cells: &TallyCells) {
        for cell in &cells.crash {
            add_n(&mut self.crash, cell.key, cell.n);
        }
        self.active |= !cells.active_week.is_empty();
    }
}
