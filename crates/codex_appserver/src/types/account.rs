//! `account/read`: `GetAccountParams`, `GetAccountResponse`, its `Account` union and `PlanType`
//! (schema `v2/`).

use serde::{Deserialize, Serialize};

use super::{open_enum, required_nullable};

/// `account/read`'s params.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetAccountParams {
    /// Asks for a token refresh before answering. [`Client`](crate::Client) never sets it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<bool>,
}

/// `account/read`'s response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetAccountResponse {
    /// The signed-in account, or `None` when nobody is signed in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<Account>,
    /// Whether the configured model provider needs an `OpenAI` sign-in.
    pub requires_openai_auth: bool,
}

/// `Account`: which kind of sign-in the server holds, told apart by its `type` field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Account {
    /// `"apiKey"`: an `OpenAI` API key.
    ApiKey,
    /// `"chatgpt"`: a `ChatGPT` sign-in.
    Chatgpt(ChatgptAccount),
    /// `"amazonBedrock"`: Amazon Bedrock credentials.
    AmazonBedrock(AmazonBedrockAccount),
    /// A `type` this crate does not name, kept as it was sent; the other fields are dropped.
    Unknown(String),
}

impl Account {
    /// Every `type` the vendored schema lists, in its order.
    pub const VALUES: &'static [&'static str] = &["apiKey", "chatgpt", "amazonBedrock"];

    /// The `type` as it appears on the wire.
    pub fn type_str(&self) -> &str {
        match self {
            Self::ApiKey => "apiKey",
            Self::Chatgpt(_) => "chatgpt",
            Self::AmazonBedrock(_) => "amazonBedrock",
            Self::Unknown(kind) => kind,
        }
    }
}

/// `ChatgptAccount`: the fields of a `ChatGPT` sign-in, besides `type`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatgptAccount {
    /// The account's email address; required, and `null` when the server does not know it.
    #[serde(deserialize_with = "required_nullable")]
    pub email: Option<String>,
    /// The `ChatGPT` plan.
    pub plan_type: PlanType,
}

/// `AmazonBedrockAccount`: the fields of an Amazon Bedrock sign-in, besides `type`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AmazonBedrockAccount {
    /// Whether Codex manages the credentials (the server's default is `false`).
    #[serde(default)]
    pub uses_codex_managed_credentials: bool,
}

impl Serialize for Account {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::Error as _;
        let fields = match self {
            Self::ApiKey | Self::Unknown(_) => serde_json::Value::Object(serde_json::Map::new()),
            Self::Chatgpt(account) => serde_json::to_value(account).map_err(S::Error::custom)?,
            Self::AmazonBedrock(account) => {
                serde_json::to_value(account).map_err(S::Error::custom)?
            }
        };
        let serde_json::Value::Object(mut fields) = fields else {
            return Err(S::Error::custom("an account's fields are not an object"));
        };
        fields.insert(
            "type".to_owned(),
            serde_json::Value::String(self.type_str().to_owned()),
        );
        fields.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Account {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error as _;
        let fields = serde_json::Map::<String, serde_json::Value>::deserialize(deserializer)?;
        let kind = match fields.get("type") {
            Some(serde_json::Value::String(kind)) => kind.clone(),
            Some(_) => return Err(D::Error::custom("an account's `type` is not a string")),
            None => return Err(D::Error::missing_field("type")),
        };
        let fields = serde_json::Value::Object(fields);
        Ok(match kind.as_str() {
            "apiKey" => Self::ApiKey,
            "chatgpt" => Self::Chatgpt(serde_json::from_value(fields).map_err(D::Error::custom)?),
            "amazonBedrock" => {
                Self::AmazonBedrock(serde_json::from_value(fields).map_err(D::Error::custom)?)
            }
            _ => Self::Unknown(kind),
        })
    }
}

open_enum! {
    /// `PlanType`: the account's `ChatGPT` plan.
    PlanType {
        /// `"free"`.
        Free = "free",
        /// `"go"`.
        Go = "go",
        /// `"plus"`.
        Plus = "plus",
        /// `"pro"`.
        Pro = "pro",
        /// `"prolite"`.
        Prolite = "prolite",
        /// `"promax"`.
        Promax = "promax",
        /// `"team"`.
        Team = "team",
        /// `"self_serve_business_prolite"`.
        SelfServeBusinessProlite = "self_serve_business_prolite",
        /// `"self_serve_business_usage_based"`.
        SelfServeBusinessUsageBased = "self_serve_business_usage_based",
        /// `"business"`.
        Business = "business",
        /// `"ent26"`.
        Ent26 = "ent26",
        /// `"enterprise_cbp_automation"`.
        EnterpriseCbpAutomation = "enterprise_cbp_automation",
        /// `"enterprise_cbp_usage_based"`.
        EnterpriseCbpUsageBased = "enterprise_cbp_usage_based",
        /// `"enterprise"`.
        Enterprise = "enterprise",
        /// `"edu"`.
        Edu = "edu",
        /// `"edu_plus"`.
        EduPlus = "edu_plus",
        /// `"edu_pro"`.
        EduPro = "edu_pro",
        /// `"unknown"`: Codex itself does not know the plan. Not [`Self::Unknown`], which holds
        /// values this crate does not know.
        CodexUnknown = "unknown",
    }
}
