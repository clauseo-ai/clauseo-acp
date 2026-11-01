//! `account/rateLimits/read` and `account/rateLimits/updated`: the params, the response, the
//! notification and the snapshot types they share (schema `v2/`).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::account::PlanType;
use super::open_enum;

/// `GetAccountRateLimitsParams`: `account/rateLimits/read`'s params. The usage reader sets
/// `exclude_reset_credit_details` to `Some(true)`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetAccountRateLimitsParams {
    /// Skips the reset-credit detail lookup, for background polls; the response still carries
    /// the available count.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclude_reset_credit_details: Option<bool>,
    /// The client supports the automatic Luna Reserve fallback.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_luna_reserve: Option<bool>,
}

/// `GetAccountRateLimitsResponse`: `account/rateLimits/read`'s response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetAccountRateLimitsResponse {
    /// The account the snapshot belongs to, when the backend says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    /// Whether ordinary included usage is allowed; `None` means unknown, not allowed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ordinary_usage_allowed: Option<bool>,
    /// The reset credits the account holds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_limit_reset_credits: Option<RateLimitResetCreditsSummary>,
    /// A backend-owned banner, kept as sent (its keys are `snake_case`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_limit_upsell: Option<serde_json::Value>,
    /// The single-bucket view, as older servers sent it.
    pub rate_limits: RateLimitSnapshot,
    /// The snapshot of each metered limit, keyed by its `limitId` (for example `codex`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_limits_by_limit_id: Option<BTreeMap<String, RateLimitSnapshot>>,
}

/// `RateLimitSnapshot`: one limit's state. In `account/rateLimits/updated` it is sparse: a
/// missing value is unknown, not cleared.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RateLimitSnapshot {
    /// The account's credits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credits: Option<CreditsSnapshot>,
    /// The member's own spend-control limit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub individual_limit: Option<SpendControlLimitSnapshot>,
    /// The metered limit's id, such as `codex`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit_id: Option<String>,
    /// The limit's display name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit_name: Option<String>,
    /// The model whose name and options describe this limit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normal_model_slug: Option<String>,
    /// The account's plan.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_type: Option<PlanType>,
    /// The first window. `primary` and `secondary` are positions, not durations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary: Option<RateLimitWindow>,
    /// Which limit was reached, if one was.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_limit_reached_type: Option<RateLimitReachedType>,
    /// The second window.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secondary: Option<RateLimitWindow>,
    /// Whether spend control stopped usage; `None` means unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spend_control_reached: Option<bool>,
}

/// `RateLimitWindow`: one usage window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RateLimitWindow {
    /// When the window resets, in Unix seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<i64>,
    /// How much of the window is used, in percent.
    pub used_percent: i32,
    /// How long the window is, in minutes; this, not the position, says session or week.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_duration_mins: Option<i64>,
}

/// `CreditsSnapshot`: the account's credits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreditsSnapshot {
    /// The balance, as the backend formats it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub balance: Option<String>,
    /// Whether the account has credits.
    pub has_credits: bool,
    /// Whether its credits are unlimited.
    pub unlimited: bool,
}

/// `SpendControlLimitSnapshot`: a spend-control limit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpendControlLimitSnapshot {
    /// The limit, as the backend formats it.
    pub limit: String,
    /// What is left, in percent.
    pub remaining_percent: i32,
    /// When it resets, in Unix seconds.
    pub resets_at: i64,
    /// What is used, as the backend formats it.
    pub used: String,
}

/// `RateLimitResetCreditsSummary`: the reset credits an account holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RateLimitResetCreditsSummary {
    /// How many are available.
    pub available_count: i64,
    /// The available credits; `None` when only the count is known, and possibly shorter than
    /// the count.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credits: Option<Vec<RateLimitResetCredit>>,
}

/// `RateLimitResetCredit`: one reset credit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RateLimitResetCredit {
    /// The backend's description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// When it expires, in Unix seconds; `None` if it does not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<i64>,
    /// When it was granted, in Unix seconds.
    pub granted_at: i64,
    /// The backend's opaque id.
    pub id: String,
    /// What it resets.
    pub reset_type: RateLimitResetType,
    /// Its state.
    pub status: RateLimitResetCreditStatus,
    /// The backend's title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

/// `AccountRateLimitsUpdatedNotification`: the params of `account/rateLimits/updated`, a sparse
/// update to merge into the last `account/rateLimits/read` response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountRateLimitsUpdatedNotification {
    /// The values that changed.
    pub rate_limits: RateLimitSnapshot,
}

open_enum! {
    /// `RateLimitReachedType`: which limit stopped usage.
    RateLimitReachedType {
        /// `"rate_limit_reached"`.
        RateLimitReached = "rate_limit_reached",
        /// `"workspace_owner_credits_depleted"`.
        WorkspaceOwnerCreditsDepleted = "workspace_owner_credits_depleted",
        /// `"workspace_member_credits_depleted"`.
        WorkspaceMemberCreditsDepleted = "workspace_member_credits_depleted",
        /// `"workspace_owner_usage_limit_reached"`.
        WorkspaceOwnerUsageLimitReached = "workspace_owner_usage_limit_reached",
        /// `"workspace_member_usage_limit_reached"`.
        WorkspaceMemberUsageLimitReached = "workspace_member_usage_limit_reached",
    }
}

open_enum! {
    /// `RateLimitResetCreditStatus`: a reset credit's state.
    RateLimitResetCreditStatus {
        /// `"available"`.
        Available = "available",
        /// `"redeeming"`.
        Redeeming = "redeeming",
        /// `"redeemed"`.
        Redeemed = "redeemed",
        /// `"unknown"`: Codex itself does not know. Not [`Self::Unknown`], which holds values
        /// this crate does not know.
        CodexUnknown = "unknown",
    }
}

open_enum! {
    /// `RateLimitResetType`: what a reset credit resets.
    RateLimitResetType {
        /// `"codexRateLimits"`.
        CodexRateLimits = "codexRateLimits",
        /// `"unknown"`: Codex itself does not know. Not [`Self::Unknown`], which holds values
        /// this crate does not know.
        CodexUnknown = "unknown",
    }
}
