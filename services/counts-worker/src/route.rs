//! The two routes, what each may carry, and the statuses the Worker answers with. Every cap here
//! is the crate's own constant ([`clauseo_counts::schema`]) except the daily caps, which are
//! the Worker's and live only here.

use clauseo_counts::schema::{
    Invalid, MAX_COUNTS_GZIP, MAX_COUNTS_JSON, MAX_TALLY_GZIP, MAX_TALLY_JSON,
};

/// The most `/v1/counts` requests the Worker stores in one UTC day, across every sender. A day
/// (not an hour) because the app sends within minutes of UTC midnight. At the cap a day writes
/// at most 3,000 × 8 rows (7 reports and the count).
pub(crate) const DAILY_CAP_COUNTS: i64 = 3_000;
/// The most `/v1/tally` requests the Worker stores in one UTC day, across every sender.
pub(crate) const DAILY_CAP_TALLY: i64 = 1_000;

/// A path the Worker answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Route {
    /// `POST /v1/counts`: 1 to 7 day reports.
    Counts,
    /// `POST /v1/tally`: one weekly tally.
    Tally,
}

/// Why a request was not stored. Each is answered with a status and an empty body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Refusal {
    /// A path the Worker does not have.
    NotFound,
    /// One of the two paths, with a method other than `POST`.
    Method,
    /// Anything wrong with the headers, the body's size, its gzip or its schema.
    Bad,
    /// The route's daily cap is spent.
    Busy,
    /// D1 or the clock failed; the sender keeps the report and tries again later.
    Failed,
}

impl Refusal {
    /// The HTTP status.
    pub(crate) const fn status(self) -> u16 {
        match self {
            Refusal::NotFound => 404,
            Refusal::Method => 405,
            Refusal::Bad => 400,
            Refusal::Busy => 429,
            Refusal::Failed => 500,
        }
    }
}

/// A failed binding, statement or body stream.
impl From<worker::Error> for Refusal {
    fn from(_: worker::Error) -> Refusal {
        Refusal::Failed
    }
}

/// Cells the crate could not write back, which its own types rule out.
impl From<serde_json::Error> for Refusal {
    fn from(_: serde_json::Error) -> Refusal {
        Refusal::Failed
    }
}

/// Anything the crate's parser refuses.
impl From<Invalid> for Refusal {
    fn from(_: Invalid) -> Refusal {
        Refusal::Bad
    }
}

/// A body that is not gzip, or a cut-off one.
impl From<std::io::Error> for Refusal {
    fn from(_: std::io::Error) -> Refusal {
        Refusal::Bad
    }
}

/// A `Content-Length` that is not a whole number.
impl From<std::num::ParseIntError> for Refusal {
    fn from(_: std::num::ParseIntError) -> Refusal {
        Refusal::Bad
    }
}

/// A cap past what the platform counts, which wasm32 and 64-bit platforms rule out.
impl From<std::num::TryFromIntError> for Refusal {
    fn from(_: std::num::TryFromIntError) -> Refusal {
        Refusal::Bad
    }
}

impl Route {
    /// The route at `path`, if `post`; [`Refusal::Method`] for another method on one of the two
    /// paths, and [`Refusal::NotFound`] for any other path. The path matches exactly: no
    /// trailing slash, no other case.
    pub(crate) fn find(post: bool, path: &str) -> Result<Route, Refusal> {
        let route = match path {
            "/v1/counts" => Route::Counts,
            "/v1/tally" => Route::Tally,
            _ => return Err(Refusal::NotFound),
        };
        if post {
            Ok(route)
        } else {
            Err(Refusal::Method)
        }
    }

    /// The largest gzip body the route takes.
    pub(crate) const fn gzip_cap(self) -> usize {
        match self {
            Route::Counts => MAX_COUNTS_GZIP,
            Route::Tally => MAX_TALLY_GZIP,
        }
    }

    /// The largest JSON the gzip body may inflate to.
    pub(crate) const fn json_cap(self) -> usize {
        match self {
            Route::Counts => MAX_COUNTS_JSON,
            Route::Tally => MAX_TALLY_JSON,
        }
    }

    /// The most requests stored in one UTC day.
    pub(crate) const fn daily_cap(self) -> i64 {
        match self {
            Route::Counts => DAILY_CAP_COUNTS,
            Route::Tally => DAILY_CAP_TALLY,
        }
    }

    /// The route's name in the `rate` table.
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Route::Counts => "counts",
            Route::Tally => "tally",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_post_on_the_two_paths_is_a_route() {
        assert_eq!(Route::find(true, "/v1/counts"), Ok(Route::Counts));
        assert_eq!(Route::find(true, "/v1/tally"), Ok(Route::Tally));
        assert_eq!(Route::find(false, "/v1/counts"), Err(Refusal::Method));
        assert_eq!(Route::find(false, "/v1/tally"), Err(Refusal::Method));
        for path in [
            "/",
            "/v1/counts/",
            "/V1/counts",
            "/v1/crash",
            "/v1/query",
            "/dash",
            "",
        ] {
            assert_eq!(Route::find(true, path), Err(Refusal::NotFound), "{path}");
            assert_eq!(Route::find(false, path), Err(Refusal::NotFound), "{path}");
        }
    }

    #[test]
    fn the_caps_are_the_crates_and_the_statuses_fixed() {
        assert_eq!(Route::Counts.gzip_cap(), 16_000);
        assert_eq!(Route::Counts.json_cap(), 64_000);
        assert_eq!(Route::Tally.gzip_cap(), 4_000);
        assert_eq!(Route::Tally.json_cap(), 16_000);
        assert_eq!(Route::Counts.daily_cap(), 3_000);
        assert_eq!(Route::Tally.daily_cap(), 1_000);
        let statuses = [
            Refusal::NotFound,
            Refusal::Method,
            Refusal::Bad,
            Refusal::Busy,
            Refusal::Failed,
        ]
        .map(Refusal::status);
        assert_eq!(statuses, [404, 405, 400, 429, 500]);
    }
}
