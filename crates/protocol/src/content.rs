//! Content blocks: what a message, a chunk or a tool call carries besides plain text.
//!
//! [`ContentBlock`] copies ACP v1's block kinds (schema 1.11.0) as Clauseo's own type, with
//! [`TextOrBlob`] for the unbounded fields. Its `Deserialize` is written by hand: a block kind a
//! newer Clauseo adds becomes [`ContentBlock::Unsupported`] with the whole block kept as compact
//! JSON text, so nothing of it is lost and a later upcaster can recover it.

use serde::de::{self, MapAccess};
use serde::{Deserialize, Deserializer, Serialize};

use crate::open_enum::{OpenDataEnum, deserialize_open};
use crate::{CompactString, FileRefId, RawPayload, TextOrBlob, ToolLocation};

/// One block of content, as ACP v1 defines it, plus `Unsupported` for a kind this version does
/// not know.
///
/// JSON: `{"text": {"text": "..."}}`, `{"image": {"data": {"inline": "<base64>"}, ...}}`.
/// `data` and `Resource.blob` hold ACP's base64 text as it came.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum ContentBlock {
    /// Plain text (markdown in an agent's answer).
    Text {
        /// The text.
        text: String,
    },
    /// An image.
    Image {
        /// The image's base64 text.
        data: TextOrBlob,
        /// Its media type, such as `image/png`.
        #[cfg_attr(feature = "specta", specta(type = String))]
        mime_type: CompactString,
        /// Where the image came from, kept as data, never fetched.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        uri: Option<String>,
    },
    /// An audio clip.
    Audio {
        /// The clip's base64 text.
        data: TextOrBlob,
        /// Its media type, such as `audio/wav`.
        #[cfg_attr(feature = "specta", specta(type = String))]
        mime_type: CompactString,
    },
    /// A link to a resource the agent can read, kept as data.
    ResourceLink {
        /// The resource's URI.
        uri: String,
        /// Its name.
        name: String,
        /// Its media type.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        #[cfg_attr(feature = "specta", specta(type = Option<String>))]
        mime_type: Option<CompactString>,
        /// A title to show.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        title: Option<String>,
        /// A description to show.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        description: Option<String>,
        /// Its size in bytes, at most [`MAX_SAFE`](crate::MAX_SAFE).
        #[serde(skip_serializing_if = "Option::is_none", default)]
        #[cfg_attr(feature = "specta", specta(type = Option<u32>))]
        size: Option<u64>,
    },
    /// A resource embedded in the message: its text or its base64 bytes.
    Resource {
        /// The resource's URI.
        uri: String,
        /// Its media type.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        #[cfg_attr(feature = "specta", specta(type = Option<String>))]
        mime_type: Option<CompactString>,
        /// Its text, for a text resource.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        text: Option<TextOrBlob>,
        /// Its base64 bytes, for a binary resource.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        blob: Option<TextOrBlob>,
    },
    /// A block kind this version does not know, kept so a later upcaster can recover it.
    Unsupported {
        /// The block's kind: ACP's `type`, or the key of Clauseo's one-key object.
        kind: String,
        /// Whose JSON `raw` holds.
        raw_from: RawFrom,
        /// The block's JSON as compact text: the whole ACP block, or the whole one-key object.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        raw: Option<TextOrBlob>,
    },
}

/// Whose JSON an [`ContentBlock::Unsupported`] block keeps. The two forms differ (ACP's `"type"`
/// field against Clauseo's one-key object), so an upcaster must know which one it reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum RawFrom {
    /// `acp` met an ACP block type it does not know.
    Acp,
    /// This version's decoder met a block kind a newer Clauseo wrote.
    Clauseo,
}

/// Content in a tool call. A text block is `Text`; `Content` holds the other block kinds. There
/// is no terminal content: Clauseo advertises no `terminal` capability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum ToolCallContent {
    /// Text, inline or in a blob. JSON: `{"text": {"inline": "..."}}`.
    Text(TextOrBlob),
    /// A non-text content block.
    Content(ContentBlock),
    /// A file edit the tool made or proposes.
    Diff {
        /// The file, inside the matter or flagged as outside it.
        path: ToolLocation,
        /// The text before; `None` for a new file.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        old: Option<TextOrBlob>,
        /// The text after.
        new: TextOrBlob,
    },
}

/// One block of a prompt the lawyer sends. Files are named by opaque ids, never paths. Unknown
/// fields are refused (the container's `deny_unknown_fields` covers each struct variant).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum PromptBlock {
    /// Text the lawyer typed.
    Text {
        /// The text.
        text: String,
    },
    /// A file the host issued a reference for.
    File {
        /// The file's reference.
        file: FileRefId,
    },
}

// Payloads as read, one per data variant; field names and attributes match `ContentBlock`'s.

#[derive(Deserialize)]
struct TextData {
    text: String,
}

#[derive(Deserialize)]
struct ImageData {
    data: TextOrBlob,
    mime_type: CompactString,
    #[serde(default)]
    uri: Option<String>,
}

#[derive(Deserialize)]
struct AudioData {
    data: TextOrBlob,
    mime_type: CompactString,
}

#[derive(Deserialize)]
struct ResourceLinkData {
    uri: String,
    name: String,
    #[serde(default)]
    mime_type: Option<CompactString>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    size: Option<u64>,
}

#[derive(Deserialize)]
struct ResourceData {
    uri: String,
    #[serde(default)]
    mime_type: Option<CompactString>,
    #[serde(default)]
    text: Option<TextOrBlob>,
    #[serde(default)]
    blob: Option<TextOrBlob>,
}

#[derive(Deserialize)]
struct UnsupportedData {
    kind: String,
    raw_from: RawFrom,
    #[serde(default)]
    raw: Option<TextOrBlob>,
}

impl ContentBlock {
    /// The catch-all for an unknown block, its JSON being `whole`.
    fn unsupported<E: de::Error>(kind: &str, whole: &RawPayload) -> Result<Self, E> {
        let text = whole.to_json_text().map_err(E::custom)?;
        Ok(Self::Unsupported {
            kind: kind.to_owned(),
            raw_from: RawFrom::Clauseo,
            raw: Some(TextOrBlob::Inline(text)),
        })
    }
}

impl OpenDataEnum for ContentBlock {
    const EXPECTING: &'static str = "a content block";
    const DATA_KINDS: &'static [&'static str] = &[
        "text",
        "image",
        "audio",
        "resource_link",
        "resource",
        "unsupported",
    ];

    fn unit(_kind: &str) -> Option<Self> {
        None
    }

    fn data<'de, A: MapAccess<'de>>(kind: &str, map: &mut A) -> Result<Option<Self>, A::Error> {
        Ok(Some(match kind {
            "text" => {
                let TextData { text } = map.next_value()?;
                Self::Text { text }
            }
            "image" => {
                let ImageData {
                    data,
                    mime_type,
                    uri,
                } = map.next_value()?;
                Self::Image {
                    data,
                    mime_type,
                    uri,
                }
            }
            "audio" => {
                let AudioData { data, mime_type } = map.next_value()?;
                Self::Audio { data, mime_type }
            }
            "resource_link" => {
                let d: ResourceLinkData = map.next_value()?;
                Self::ResourceLink {
                    uri: d.uri,
                    name: d.name,
                    mime_type: d.mime_type,
                    title: d.title,
                    description: d.description,
                    size: d.size,
                }
            }
            "resource" => {
                let d: ResourceData = map.next_value()?;
                Self::Resource {
                    uri: d.uri,
                    mime_type: d.mime_type,
                    text: d.text,
                    blob: d.blob,
                }
            }
            "unsupported" => {
                let d: UnsupportedData = map.next_value()?;
                Self::Unsupported {
                    kind: d.kind,
                    raw_from: d.raw_from,
                    raw: d.raw,
                }
            }
            _ => return Ok(None),
        }))
    }

    fn unknown(kind: &str) -> Self {
        // A plain string: a unit kind a newer Clauseo added. Its JSON is the string itself, which
        // always has a JSON form.
        let raw = RawPayload::Str(kind.to_owned()).to_json_text().ok();
        Self::Unsupported {
            kind: kind.to_owned(),
            raw_from: RawFrom::Clauseo,
            raw: raw.map(TextOrBlob::Inline),
        }
    }

    fn unknown_data<'de, A: MapAccess<'de>>(kind: &str, map: &mut A) -> Result<Self, A::Error> {
        let payload: RawPayload = map.next_value()?;
        let whole = RawPayload::Map(vec![(RawPayload::Str(kind.to_owned()), payload)]);
        Self::unsupported(kind, &whole)
    }
}

impl<'de> Deserialize<'de> for ContentBlock {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserialize_open(deserializer)
    }
}
