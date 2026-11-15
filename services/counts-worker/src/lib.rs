//! The Cloudflare Worker behind `t.clauseo.chat` (decision 0056, `docs/counts-worker.md`). It
//! takes the anonymous usage counts the app sends, `POST /v1/counts` (1 to 7 day reports) and
//! `POST /v1/tally` (one weekly tally), and crash reports, `POST /v1/crash` (one report), checks
//! them with `clauseo-counts`' own schema, and keeps the validated value in D1, and each crash
//! report's JSON in R2. It answers `204`, or a status with an empty body. A daily cron
//! fills the `install_week` and `rollup` summaries and deletes rows past their time; the
//! `v_*` views (migrations `0002_rollup.sql` and `0004_crash.sql`) read them with every group
//! under 5 hidden. Behind Cloudflare Access, `GET /dash` shows Rohan six charts and
//! `POST /v1/query` runs one guarded query over the views (decision 0061).
//!
//! What it never does: read a request header other than `Content-Encoding`, `Content-Length`
//! and `Cf-Access-Jwt-Assertion`, read the sender's address or country, keep the time a request
//! arrived, or write a log line.

mod access;
mod body;
mod crash;
mod dash;
mod glue;
mod query;
mod read;
mod retention;
mod rollup;
mod route;
mod rows;
mod webcrypto;
