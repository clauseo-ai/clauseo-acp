//! The one list of the enums a log entry can hold (every enum reachable from `Envelope` and
//! `DomainEvent`), for the tests that check 0002's enum table and the corpus against it.
//!
//! `logged_enums!(rows)` writes `enum_rows!(..)` with the whole list,
//! `logged_enums!(data_coverage)` writes `data_coverage!(..)` with the open enums with data, and
//! `logged_enums!(unit_coverage)` writes `unit_coverage!(..)` with the open unit-only ones; the
//! file that asks defines that macro.
//! The called names are written here, never passed in. The list's form:
//!
//! ```text
//! event: DomainEvent(Unknown);
//! data: <open enum with data>(<catch-all variant>), ...;
//! unit: <open unit-only enum>, ...;
//! closed: <closed enum>, ...;
//! ```
//!
//! A unit-only enum's catch-all is `Unknown`. A test file includes it as below.
//!
//! ```text
//! #[cfg(test)]
//! #[macro_use]
//! #[path = "support/enums.rs"]
//! mod enums;
//! ```

/// Writes the macro call `$which` names with every logged enum; see the module doc.
macro_rules! logged_enums {
    (@list rows;
        event: $event:ident($event_catch:ident);
        data: $($data:ident($catch:ident)),+;
        unit: $($unit:ident),+;
        closed: $($closed:ident),+;
    ) => {
        enum_rows!($event($event_catch); $($data($catch)),+; $($unit),+; $($closed),+)
    };
    (@list data_coverage;
        event: $event:ident($event_catch:ident);
        data: $($data:ident($catch:ident)),+;
        unit: $($unit:ident),+;
        closed: $($closed:ident),+;
    ) => {
        data_coverage!($($data),+)
    };
    (@list unit_coverage;
        event: $event:ident($event_catch:ident);
        data: $($data:ident($catch:ident)),+;
        unit: $($unit:ident),+;
        closed: $($closed:ident),+;
    ) => {
        unit_coverage!($($unit),+)
    };
    ($which:ident) => {
        logged_enums! { @list $which;
            event: DomainEvent(Unknown);
            data:
                ContentBlock(Unsupported),
                DetachReason(Unknown),
                SteerOutcome(Unknown),
                Principal(Unknown),
                ToolCallContent(Unknown),
                ToolLocation(Unknown),
                PermissionChoice(Unknown),
                ElicitationRequest(Unknown),
                ConfigOptionKind(Unknown),
                ChangeKind(Unknown),
                RestoreScope(Unknown);
            unit:
                ErrorCode,
                ToolKind,
                ToolCallStatus,
                PermissionKind,
                StopReason,
                NoticeLevel,
                Deliverable,
                CheckpointKind,
                ExpiryReason,
                RenameSource,
                Delivery,
                SkippedFileReason,
                TextKind,
                RawFrom,
                ElicitationOutcome,
                PlanPriority,
                PlanStatus,
                ConfigCategory,
                CompactionStatus,
                LimitStopReason,
                MatterKind;
            closed: TextOrBlob, RawPayload, StreamId;
        }
    };
}
