//! The Cloudflare Worker behind `t.clauseo.chat` (decision 0056, `docs/counts-worker.md`). It
//! takes the anonymous usage counts the app sends, `POST /v1/counts` (1 to 7 day reports) and
//! `POST /v1/tally` (one weekly tally), checks them with `clauseo-counts`' own schema, and keeps
//! the validated value in D1. It answers `204`, or a status with an empty body.
//!
//! What it never does: read a request header other than `Content-Encoding` and
//! `Content-Length`, read the sender's address or country, keep the time a request arrived, or
//! write a log line.

mod body;
mod glue;
mod retention;
mod route;
mod rows;
