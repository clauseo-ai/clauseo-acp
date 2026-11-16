//! Reads what `stetdoc compare --json` prints beside the redline it writes: its receipt's
//! warnings, for `run.rs`. Pure, no I/O. The fields were taken from stetdoc 0.9.3's output
//! (`tests/fixtures/blackline/partial/compare-*.json`) and its source (`document/compare.rs`,
//! `receipt_of`, which lists each block compare could not mark up, its receipt's exclusions, as
//! a warning of its own).
//!
//! It answers what each warning means for the blackline, by its code:
//! - `compare_story_unmatched`: headers or footers compare did not compare (a slot one version
//!   has and the other lacks, or one with no one-to-one partner), so the redline marks nothing
//!   for them ([`BlacklineNotice::HeaderOrFooterNotCompared`]);
//! - `compare_not_redlined` and `compare_not_restored`, one for each block compare could not
//!   mark up: that block is shown as the newer version has it, or rejecting its changes would
//!   not give the older one back, so the blackline may differ ([`BlacklineNotice::MayDiffer`]);
//! - `compare_revisions_accepted` (the versions' own tracked changes accepted first),
//!   `compare_paragraph_replaced` and `compare_table_replaced` (shown as deleted and inserted)
//!   and `compare_keys_merged` (a note for stetdoc's own check): none, since the blackline still
//!   shows every change;
//! - any other code (something in deleted content compare could not carry, such as a picture or
//!   a note, or a code this version does not know): the blackline may differ.
//!
//! An answer that is not a receipt with a list of warnings may differ too: what compare said
//! can't be read, so it is never taken as "nothing to say". The log gets the codes, never a
//! warning's message, which may quote the document.

use clauseo_protocol::BlacklineNotice;
use serde::Deserialize;

use super::code_word;

/// The fields of compare's JSON the read needs; the others are ignored.
#[derive(Deserialize)]
struct Compared {
    receipt: Receipt,
}

/// The receipt: only its warnings are read.
#[derive(Deserialize)]
struct Receipt {
    warnings: Vec<Warning>,
}

/// One of compare's warnings; only its code is read.
#[derive(Deserialize)]
struct Warning {
    code: String,
}

/// What compare's answer says for the blackline: its notices, in no order, and its warnings'
/// codes for the log (`other` for one that is not a plain word, `unreadable` for an answer that
/// does not read).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct CompareSaid {
    /// The notices its warnings give.
    pub(crate) notices: Vec<BlacklineNotice>,
    /// Its warnings' codes.
    pub(crate) codes: Vec<String>,
}

/// What `json`, the stdout of `stetdoc compare --json` that exited 0, says for the blackline.
pub(crate) fn read_compare(json: &str) -> CompareSaid {
    let Ok(compared) = serde_json::from_str::<Compared>(json) else {
        return CompareSaid {
            notices: vec![BlacklineNotice::MayDiffer],
            codes: vec!["unreadable".to_owned()],
        };
    };
    let codes: Vec<String> = compared
        .receipt
        .warnings
        .iter()
        .map(|warning| code_word(&warning.code))
        .collect();
    let notices = codes.iter().filter_map(|code| notice_of(code)).collect();
    CompareSaid { notices, codes }
}

/// The notice one of compare's warnings gives, by its code (the module doc).
fn notice_of(code: &str) -> Option<BlacklineNotice> {
    match code {
        "compare_story_unmatched" => Some(BlacklineNotice::HeaderOrFooterNotCompared),
        "compare_revisions_accepted"
        | "compare_paragraph_replaced"
        | "compare_table_replaced"
        | "compare_keys_merged" => None,
        _ => Some(BlacklineNotice::MayDiffer),
    }
}

#[cfg(test)]
mod tests;
