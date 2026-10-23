//! Content blocks: what a message, a chunk or a tool call carries besides plain text.
//!
//! [`ContentBlock`] copies ACP v1's block kinds (schema 1.11.0) as Clauseo's own type, with
//! [`TextOrBlob`] for the unbounded fields. A block kind a newer Clauseo adds decodes to
//! [`ContentBlock::Unsupported`] with the whole block kept as compact JSON text when it fits
//! [`INLINE_LIMIT_BYTES`], so a later upcaster can recover it; the log keeps the original event
//! either way. A block whose payload nests [`MAX_RAW_DEPTH`] lists or maps fails to decode, since
//! the whole block it keeps nests one level more; a newer Clauseo encodes every `RawPayload` at
//! most [`MAX_RAW_ENCODE_DEPTH`] deep, 8 levels under that. A block holding a list or a map as a
//! map key fails to decode too.

use serde::de::MapAccess;
use serde::{Deserialize, Serialize};

use crate::open_enum::{CatchAll, open_data_enum, open_unit_enum};
use crate::raw::{AsDecoded, RawSeed};
use crate::{
    CompactString, Count, FileRefId, INLINE_LIMIT_BYTES, RawPayload, TextOrBlob, ToolLocation,
    escaped_len,
};
#[cfg(doc)]
use crate::{MAX_RAW_DEPTH, MAX_RAW_ENCODE_DEPTH};

open_data_enum! {
    /// One block of content, as ACP v1 defines it, plus `Unsupported` for a kind this version
    /// does not know.
    ///
    /// JSON: `{"text": {"text": "..."}}`, `{"image": {"data": {"inline": "<base64>"}, ...}}`.
    /// `data` and `Resource.blob` hold ACP's base64 text as it came.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum ContentBlock {
        /// Plain text (markdown in an agent's answer).
        Text = "text" {
            /// The text.
            text: String,
        },
        /// An image.
        Image = "image" {
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
        Audio = "audio" {
            /// The clip's base64 text.
            data: TextOrBlob,
            /// Its media type, such as `audio/wav`.
            #[cfg_attr(feature = "specta", specta(type = String))]
            mime_type: CompactString,
        },
        /// A link to a resource the agent can read, kept as data.
        ResourceLink = "resource_link" {
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
            size: Option<Count>,
        },
        /// A resource embedded in the message: its text or its base64 bytes.
        Resource = "resource" {
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
        /// It is the catch-all: a kind this version does not know decodes to it.
        Unsupported = "unsupported" {
            /// The block's kind: ACP's `type`, or the key of Clauseo's one-key object.
            kind: String,
            /// Whose JSON `raw` holds.
            raw_from: RawFrom,
            /// The block's JSON as compact text: the whole ACP block, or the whole one-key
            /// object. A block this version read from `MessagePack` is written as `serde_json`
            /// writes its values, which is not Clauseo's JSON form: ids and `BlobHash`es come
            /// out as arrays of byte numbers, and a scalar or binary map key as a string holding
            /// its JSON text. This version's decoder fills it when the text's [`escaped_len`] is
            /// at most [`INLINE_LIMIT_BYTES`], since the block lands in a view entry whose inline
            /// parts share that limit and that cannot be split; past it `raw` is `None`, and the
            /// log keeps the original event. That cap is only half of the rule: it sees one
            /// block, and an entry may hold several, so the reducer drops `raw` from the entry's
            /// unknown blocks, last block first, until the entry fits (0002, "Inline limit").
            /// `None` too in a block built without it.
            #[serde(skip_serializing_if = "Option::is_none", default)]
            raw: Option<TextOrBlob>,
        },
    }
    test mod content_block with [
        ContentBlock::Text {
            text: "hi".to_owned(),
        },
        ContentBlock::Image {
            data: TextOrBlob::Inline("iVBO".to_owned()),
            mime_type: CompactString::const_new("image/png"),
            uri: Some("file:///a.png".to_owned()),
        },
        ContentBlock::Audio {
            data: TextOrBlob::Inline("UklG".to_owned()),
            mime_type: CompactString::const_new("audio/wav"),
        },
        ContentBlock::ResourceLink {
            uri: "file:///a.pdf".to_owned(),
            name: "a.pdf".to_owned(),
            mime_type: Some(CompactString::const_new("application/pdf")),
            title: None,
            description: None,
            size: None,
        },
        ContentBlock::Resource {
            uri: "file:///a.md".to_owned(),
            mime_type: None,
            text: Some(TextOrBlob::Inline("x".to_owned())),
            blob: None,
        },
        ContentBlock::Unsupported {
            kind: "hologram".to_owned(),
            raw_from: RawFrom::Acp,
            raw: None,
        },
    ];
}

open_unit_enum! {
    /// Whose JSON an [`ContentBlock::Unsupported`] block keeps. The two forms differ (ACP's
    /// `"type"` field against Clauseo's one-key object), so an upcaster must know which one it
    /// reads.
    pub enum RawFrom {
        /// `acp` met an ACP block type it does not know.
        Acp,
        /// This version's decoder met a block kind a newer Clauseo wrote.
        Clauseo,
    }
    test mod raw_from;
}

open_data_enum! {
    /// Content in a tool call, as ACP v1's `ToolCallContent`: `Text` for a text block, `Content`
    /// for the other block kinds, `Diff` for a file edit. There is no terminal content: Clauseo
    /// advertises no `terminal` capability; a kind from a newer Clauseo decodes to `Unknown`.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum ToolCallContent {
        /// Text, inline or in a blob. JSON: `{"text": {"text": {"inline": "..."}}}`; the field
        /// is `text` as in `ContentBlock::Text` and ACP's text block.
        Text = "text" {
            /// The text.
            text: TextOrBlob,
        },
        /// A non-text content block. JSON: `{"content": {"content": {"image": {..}}}}`, ACP's
        /// `{"type": "content", "content": {..}}`.
        Content = "content" {
            /// The block.
            content: ContentBlock,
        },
        /// A file edit the tool made or proposes.
        Diff = "diff" {
            /// The file, inside the matter or flagged as outside it: a `ToolLocation`, not a
            /// `MatterPath`, since a tool may edit a file outside the matter (0002).
            path: ToolLocation,
            /// The text before; `None` for a new file.
            #[serde(skip_serializing_if = "Option::is_none", default)]
            old: Option<TextOrBlob>,
            /// The text after.
            new: TextOrBlob,
        },
        _ =>
        /// Content from a newer Clauseo; written back as the plain string `kind`.
        Unknown,
    }
    test mod tool_call_content with [
        ToolCallContent::Text {
            text: TextOrBlob::Inline("out".to_owned()),
        },
        ToolCallContent::Content {
            content: ContentBlock::Text {
                text: "hi".to_owned(),
            },
        },
        ToolCallContent::Diff {
            path: ToolLocation::Outside {
                display: "~/a.txt".to_owned(),
            },
            old: Some(TextOrBlob::Inline("old".to_owned())),
            new: TextOrBlob::Inline("new".to_owned()),
        },
    ];
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

impl ContentBlock {
    /// The catch-all for an unknown block, its value being `whole`. `whole` was decoded, so it
    /// nests at most [`MAX_RAW_DEPTH`] and holds no list or map as a map key (decoding refuses
    /// one, failing the event): its JSON copy, written under the decode limit, never fails.
    /// `raw` keeps that copy when its [`escaped_len`] is at most [`INLINE_LIMIT_BYTES`]. That
    /// caps this block alone; the reducer makes the whole entry fit (see `Unsupported.raw`).
    fn unsupported(kind: &str, whole: &RawPayload) -> Self {
        let json = serde_json::to_string(&AsDecoded(whole)).ok();
        Self::Unsupported {
            kind: kind.to_owned(),
            raw_from: RawFrom::Clauseo,
            raw: json
                .filter(|json| escaped_len(json) <= INLINE_LIMIT_BYTES)
                .map(TextOrBlob::Inline),
        }
    }
}

impl CatchAll for ContentBlock {
    fn unknown(kind: &str) -> Self {
        // A plain string: a unit kind a newer Clauseo added. Its JSON is the string itself, which
        // always has a JSON form, kept under the same size rule.
        Self::unsupported(kind, &RawPayload::Str(kind.into()))
    }

    fn unknown_data<'de, A: MapAccess<'de>>(kind: &str, map: &mut A) -> Result<Self, A::Error> {
        // Read one level down, as it sits inside `whole`, so what decodes also encodes under the
        // decode limit. A payload at `MAX_RAW_DEPTH` fails the event; a newer Clauseo encodes
        // every `RawPayload` at most `MAX_RAW_ENCODE_DEPTH` deep, 8 levels under it.
        let payload = map.next_value_seed(RawSeed { depth: 1 })?;
        let whole = RawPayload::Map(vec![(RawPayload::Str(kind.into()), payload)].into());
        Ok(Self::unsupported(kind, &whole))
    }
}
