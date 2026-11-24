//! `model/list`: the models Codex offers the signed-in account, a page at a time, each with its
//! reasoning efforts. Named as `v2/ModelListParams.json` and `v2/ModelListResponse.json` at
//! openai/codex tag `rust-v0.171.0`. Only the fields the model probe reads are mirrored; the rest
//! are ignored.

use serde::{Deserialize, Serialize};

/// `model/list`'s params: the page to read.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelListParams {
    /// The `nextCursor` of the page before; `None` for the first page.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub cursor: Option<String>,
    /// Whether models hidden from the default picker are listed too.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub include_hidden: Option<bool>,
    /// The page size; the server picks one when `None`.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub limit: Option<u32>,
    /// A configured provider to list the models of, instead of the app-server's default.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub model_provider: Option<String>,
}

/// One page of `model/list`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelListResponse {
    /// The page's models.
    pub data: Vec<Model>,
    /// The cursor of the next page; `None` on the last.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub next_cursor: Option<String>,
}

/// A model, as `model/list` lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Model {
    /// Its id, the value a session's model option takes.
    pub id: String,
    /// The model's own name (`model` on the wire).
    #[serde(rename = "model")]
    pub slug: String,
    /// Its display name.
    pub display_name: String,
    /// Its description.
    pub description: String,
    /// Whether it is hidden from the default picker.
    pub hidden: bool,
    /// Whether it is the account's default model.
    pub is_default: bool,
    /// Its reasoning efforts, in Codex's order.
    pub supported_reasoning_efforts: Vec<ReasoningEffortOption>,
    /// The effort Codex uses for it by default.
    pub default_reasoning_effort: String,
}

/// One reasoning effort of a model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningEffortOption {
    /// The effort's value (`low`, `medium`, ...).
    pub reasoning_effort: String,
    /// Its description.
    pub description: String,
}
