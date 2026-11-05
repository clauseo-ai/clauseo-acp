//! The buckets: a duration or a count turned into one of a few names, so no exact figure travels.

use super::closed::closed_enum;

closed_enum! {
    "How long something took (`D` in research 37 §2.2)."
    pub enum Took {
        "Under 1 second."
        Lt1s = "lt_1s",
        "Under 3 seconds."
        Lt3s = "lt_3s",
        "Under 10 seconds."
        Lt10s = "lt_10s",
        "Under 30 seconds."
        Lt30s = "lt_30s",
        "Under a minute."
        Lt1m = "lt_1m",
        "Under 3 minutes."
        Lt3m = "lt_3m",
        "Under 10 minutes."
        Lt10m = "lt_10m",
        "Under 30 minutes."
        Lt30m = "lt_30m",
        "Under an hour."
        Lt1h = "lt_1h",
        "An hour or more."
        Ge1h = "ge_1h",
    }
}

impl Took {
    /// The bucket for a duration in milliseconds.
    pub const fn from_millis(ms: u64) -> Took {
        const S: u64 = 1000;
        match ms {
            ms if ms < S => Took::Lt1s,
            ms if ms < 3 * S => Took::Lt3s,
            ms if ms < 10 * S => Took::Lt10s,
            ms if ms < 30 * S => Took::Lt30s,
            ms if ms < 60 * S => Took::Lt1m,
            ms if ms < 180 * S => Took::Lt3m,
            ms if ms < 600 * S => Took::Lt10m,
            ms if ms < 1800 * S => Took::Lt30m,
            ms if ms < 3600 * S => Took::Lt1h,
            _ => Took::Ge1h,
        }
    }
}

closed_enum! {
    "How long the app was in the foreground in one UTC day (`time_open`)."
    pub enum TimeOpenBucket {
        "Under 15 minutes."
        Lt15m = "lt_15m",
        "Under an hour."
        Lt1h = "lt_1h",
        "Under 3 hours."
        Lt3h = "lt_3h",
        "Under 6 hours."
        Lt6h = "lt_6h",
        "6 hours or more."
        Ge6h = "ge_6h",
    }
}

impl TimeOpenBucket {
    /// The bucket for a number of seconds.
    pub const fn from_secs(secs: u64) -> TimeOpenBucket {
        const H: u64 = 3600;
        match secs {
            s if s < 15 * 60 => TimeOpenBucket::Lt15m,
            s if s < H => TimeOpenBucket::Lt1h,
            s if s < 3 * H => TimeOpenBucket::Lt3h,
            s if s < 6 * H => TimeOpenBucket::Lt6h,
            _ => TimeOpenBucket::Ge6h,
        }
    }
}

closed_enum! {
    "How many papers a matter holds, as a bucket."
    pub enum Papers {
        "None."
        None = "0",
        "1 to 10."
        UpTo10 = "1_10",
        "11 to 50."
        UpTo50 = "11_50",
        "51 to 200."
        UpTo200 = "51_200",
        "201 to 1,000."
        UpTo1000 = "201_1000",
        "More than 1,000."
        Over1000 = "over_1000",
    }
}

impl Papers {
    /// The bucket for a count of papers.
    pub const fn from_count(count: u64) -> Papers {
        match count {
            0 => Papers::None,
            1..=10 => Papers::UpTo10,
            11..=50 => Papers::UpTo50,
            51..=200 => Papers::UpTo200,
            201..=1000 => Papers::UpTo1000,
            _ => Papers::Over1000,
        }
    }
}

closed_enum! {
    "How long a hang lasted (`stall.took`)."
    pub enum StallTook {
        "100 ms or more."
        Ge100ms = "ge_100ms",
        "250 ms or more."
        Ge250ms = "ge_250ms",
        "A second or more."
        Ge1s = "ge_1s",
        "5 seconds or more."
        Ge5s = "ge_5s",
    }
}

impl StallTook {
    /// The bucket for a hang in milliseconds, or `None` under 100 ms, which is not a stall.
    pub const fn from_millis(ms: u64) -> Option<StallTook> {
        match ms {
            0..100 => None,
            100..250 => Some(StallTook::Ge100ms),
            250..1000 => Some(StallTook::Ge250ms),
            1000..5000 => Some(StallTook::Ge1s),
            _ => Some(StallTook::Ge5s),
        }
    }
}
