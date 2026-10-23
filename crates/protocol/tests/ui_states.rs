//! `docs/ui-states.md` names every variant the UI must have a state for. This test keeps the list
//! true: each variant of the listed enums appears in the document's sections 1 to 9, as
//! `` `Type::Variant` `` in the second column of a table row. The tables are read as a reader
//! sees them (`support/markdown.rs`), so a row in a code block or an HTML comment names nothing.

#![forbid(clippy::disallowed_methods, clippy::disallowed_types)]

#[cfg(test)]
#[path = "support/markdown.rs"]
pub mod markdown;

use std::collections::BTreeSet;
use std::ops::RangeInclusive;

use clauseo_protocol::{
    CheckpointKind, Deliverable, Delivery, ElicitationOutcome, ErrorCode, ExpiryReason,
    LimitSource, NoticeLevel, PermissionKind, SkippedFileReason, SteerOutcome, StopReason,
    ThreadStatus, ToolCallStatus, ToolKind,
};

/// The document, read at build time.
const UI_STATES: &str = include_str!("../../../docs/ui-states.md");

/// The sections (`## 1.` to `## 9.`) whose tables name the variants. Later sections are plans.
const GUARDED_SECTIONS: RangeInclusive<u32> = 1..=9;

/// The number that a `N. Title` heading starts with.
fn section_number(heading: &str) -> Option<u32> {
    let (number, _) = heading.split_once(". ")?;
    number.parse().ok()
}

/// Whether a code span is `Type::Variant`: two words of letters and digits joined by `::`.
fn is_type_and_variant(span: &str) -> bool {
    span.split_once("::").is_some_and(|(ty, variant)| {
        [ty, variant]
            .iter()
            .all(|word| !word.is_empty() && word.chars().all(|c| c.is_ascii_alphanumeric()))
    })
}

/// The `Type::Variant` names in the second column of the table rows of a guarded section.
fn guarded_names(doc: &str) -> BTreeSet<String> {
    let in_guarded =
        |heading: &str| section_number(heading).is_some_and(|n| GUARDED_SECTIONS.contains(&n));
    markdown::tables_where(doc, in_guarded)
        .iter()
        .flat_map(|table| &table.rows)
        .filter_map(|row| row.get(1))
        .flat_map(|cell| &cell.code)
        .filter(|span| is_type_and_variant(span))
        .cloned()
        .collect()
}

/// `Type::Variant` for every variant of each enum named: `data:` enums through their
/// `VARIANT_NAMES`, `unit:` enums through the names of their `ALL`.
macro_rules! every_variant {
    (data: $($data:ident),+; unit: $($unit:ident),+ $(,)?) => {{
        let mut every: Vec<String> = Vec::new();
        $(every.extend($data::VARIANT_NAMES.iter().map(|v| format!("{}::{v}", stringify!($data))));)+
        $(every.extend($unit::ALL.iter().map(|v| {
            format!("{}::{}", stringify!($unit), $unit::variant_name(v))
        }));)+
        every
    }};
}

#[test]
fn every_listed_variant_is_in_ui_states() {
    let every = every_variant!(
        data: ThreadStatus, LimitSource, SteerOutcome;
        unit: StopReason, ToolKind, ToolCallStatus, PermissionKind, ExpiryReason,
            ElicitationOutcome, CheckpointKind, SkippedFileReason, Delivery, ErrorCode,
            Deliverable, NoticeLevel,
    );
    let guarded = guarded_names(UI_STATES);
    let missing: Vec<String> = every
        .into_iter()
        .filter(|name| !guarded.contains(name))
        .collect();
    assert!(
        missing.is_empty(),
        "docs/ui-states.md does not list these variants as `Type::Variant` in the second column \
         of a table row in sections 1 to 9: {}",
        missing.join(", ")
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A row in section 3 counts; the same name in prose, in column three, in a header row, in
    /// a code fence, in an HTML comment, in section 10 or under an unnumbered heading does not.
    const SAMPLE: &str = "\
# Title

## 3. Step card

Prose names `StopReason::Refusal` here.

| State | Type and variant | Data |
|---|---|---|
| Finished | `StopReason::EndTurn` | files |
| Needs you | `ViewEntry::Permission` or `ViewEntry::Elicitation` | the request |
| Column three | ok | `StopReason::MaxTokens` |

| State | `StopReason::Failed` | Data |
|---|---|---|
| Short row |

```
| Fenced | `StopReason::Refusal` | x |
```

<!--
| Commented | `StopReason::Cancelled` | x |
-->

## 10. Planned

| State | Type and variant | Data |
|---|---|---|
| Later | `StopReason::Unknown` | x |

## Notes

| State | Type and variant | Data |
|---|---|---|
| Unnumbered | `ToolKind::Read` | x |
";

    #[test]
    fn only_second_column_rows_of_guarded_sections_count() {
        let expected = [
            "StopReason::EndTurn",
            "ViewEntry::Permission",
            "ViewEntry::Elicitation",
        ];
        assert_eq!(
            guarded_names(SAMPLE),
            expected.iter().map(|name| (*name).to_owned()).collect()
        );
    }

    #[test]
    fn section_numbers_are_read_from_the_heading() {
        assert_eq!(section_number("1. Thread status"), Some(1));
        assert_eq!(section_number("10. Planned"), Some(10));
        assert_eq!(section_number("Notes"), None);
        assert_eq!(section_number("3 Step card"), None);
    }

    #[test]
    fn only_type_and_variant_spans_are_names() {
        assert!(is_type_and_variant("ToolKind::Read"));
        assert!(!is_type_and_variant("a::b::c"));
        assert!(!is_type_and_variant("::Read"));
        assert!(!is_type_and_variant("ToolKind::"));
        assert!(!is_type_and_variant("AgentLimitsView.meter"));
    }

    /// The cases the hand-written parsers got wrong, each now right by construction.
    #[test]
    fn the_markdown_reader_sees_the_tables_a_reader_sees() {
        let doc = "\
## Codes

| code | meaning | retryable |
|---|---|---|
| `a.b` | A `x`, `y` and z. | yes |
| `short` |

```text
| `fenced.row` | in a fence | no |
```

    | `indented.row` | indented code | no |

## Other

| code | meaning | retryable |
|---|---|---|
| `elsewhere` | another section | no |
";
        let table = markdown::table(doc, "Codes", "code");
        let codes: Vec<&Cell> = table.cells("code");
        let names: Vec<&str> = codes
            .iter()
            .flat_map(|c| &c.code)
            .map(String::as_str)
            .collect();
        assert_eq!(names, ["a.b", "short"]);
        let meaning = table.cells("meaning");
        assert_eq!(meaning[0].text, "A `x`, `y` and z.");
        assert_eq!(meaning[0].code, ["x", "y"]);
        // A short row has an empty cell for each column it leaves out.
        assert_eq!(table.rows[1].len(), 3);
        assert_eq!(table.cells("retryable")[1].text, "");
        assert_eq!(table.cells("retryable")[0].text, "yes");
    }

    use markdown::Cell;
}
