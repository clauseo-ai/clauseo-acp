use clauseo_protocol::{ErrorCode, HostError, Principal};

use super::{Access, ENTRY_POINTS, authorize};

/// Fails to compile when `Access` gains a variant, so `ENTRY_POINTS` is checked against it.
fn listed(access: Access) -> bool {
    match access {
        Access::Dispatch
        | Access::Query
        | Access::Subscribe
        | Access::SubscribeEvents
        | Access::Ack
        | Access::RegisterFolder
        | Access::RegisterFile
        | Access::Shutdown => ENTRY_POINTS.contains(&access),
    }
}

#[test]
fn every_entry_point_checks() {
    let phone: Principal = serde_json::from_str("\"some_phone\"").unwrap();
    assert!(matches!(phone, Principal::Unknown { .. }));
    for access in ENTRY_POINTS {
        authorize(&Principal::LocalDesktop, *access).unwrap();
        let refused = HostError::from(authorize(&phone, *access).unwrap_err());
        assert_eq!(
            refused.code(),
            ErrorCode::TransportUnauthorized,
            "{access:?}"
        );
        assert!(!refused.retryable());
    }
}

#[test]
fn entry_points_name_every_access() {
    let all = [
        Access::Dispatch,
        Access::Query,
        Access::Subscribe,
        Access::SubscribeEvents,
        Access::Ack,
        Access::RegisterFolder,
        Access::RegisterFile,
        Access::Shutdown,
    ];
    assert!(all.into_iter().all(listed));
    assert_eq!(ENTRY_POINTS.len(), all.len());
}
