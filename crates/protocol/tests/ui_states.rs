//! `docs/ui-states.md` names every variant the UI must have a state for. This test keeps the list
//! true: each variant of the listed enums appears in the document as `` `Type::Variant` ``.

use clauseo_protocol::{
    CheckpointKind, Delivery, ElicitationOutcome, ErrorCode, ExpiryReason, LimitSource,
    PermissionKind, SkippedFileReason, SteerOutcome, StopReason, ThreadStatus, ToolCallStatus,
    ToolKind,
};

/// The document, read at build time.
const UI_STATES: &str = include_str!("../../../docs/ui-states.md");

/// The `` `Type::Variant` `` name of every value in `all`, named by `variant_name`.
fn listed<T>(ty: &str, all: &[T], variant_name: fn(&T) -> &'static str) -> Vec<String> {
    all.iter()
        .map(|value| format!("{ty}::{}", variant_name(value)))
        .collect()
}

#[test]
fn every_listed_variant_is_in_ui_states() {
    let mut every = Vec::new();
    every.extend(listed(
        "ThreadStatus",
        &ThreadStatus::ALL,
        ThreadStatus::variant_name,
    ));
    every.extend(listed(
        "StopReason",
        &StopReason::ALL,
        StopReason::variant_name,
    ));
    every.extend(listed("ToolKind", &ToolKind::ALL, ToolKind::variant_name));
    every.extend(listed(
        "ToolCallStatus",
        &ToolCallStatus::ALL,
        ToolCallStatus::variant_name,
    ));
    every.extend(listed(
        "PermissionKind",
        &PermissionKind::ALL,
        PermissionKind::variant_name,
    ));
    every.extend(listed(
        "ExpiryReason",
        &ExpiryReason::ALL,
        ExpiryReason::variant_name,
    ));
    every.extend(listed(
        "ElicitationOutcome",
        &ElicitationOutcome::ALL,
        ElicitationOutcome::variant_name,
    ));
    every.extend(listed(
        "CheckpointKind",
        &CheckpointKind::ALL,
        CheckpointKind::variant_name,
    ));
    every.extend(listed(
        "SkippedFileReason",
        &SkippedFileReason::ALL,
        SkippedFileReason::variant_name,
    ));
    every.extend(listed(
        "LimitSource",
        &LimitSource::ALL,
        LimitSource::variant_name,
    ));
    every.extend(listed("Delivery", &Delivery::ALL, Delivery::variant_name));
    every.extend(listed(
        "SteerOutcome",
        &SteerOutcome::ALL,
        SteerOutcome::variant_name,
    ));
    every.extend(listed(
        "ErrorCode",
        &ErrorCode::ALL,
        ErrorCode::variant_name,
    ));

    let missing: Vec<String> = every
        .into_iter()
        .filter(|name| !UI_STATES.contains(&format!("`{name}`")))
        .collect();
    assert!(
        missing.is_empty(),
        "docs/ui-states.md does not list these variants as `Type::Variant`: {}",
        missing.join(", ")
    );
}
