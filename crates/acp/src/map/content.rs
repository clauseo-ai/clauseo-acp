//! Content blocks, both ways.

use agent_client_protocol::schema::v1 as acp;
use clauseo_protocol::{ContentBlock, Count, PromptCapabilities, RawFrom, TextOrBlob};
use serde_json::Value;

use crate::Error;

/// The most text one chunk update carries: 1 MiB.
pub(crate) const MAX_TEXT_CHUNK_BYTES: usize = 1024 * 1024;

/// An ACP block as `protocol`'s. Text, data and URIs move out of the SDK's type, never copied.
pub(crate) fn block(block: acp::ContentBlock) -> ContentBlock {
    match block {
        acp::ContentBlock::Text(text) => ContentBlock::Text { text: text.text },
        acp::ContentBlock::Image(image) => ContentBlock::Image {
            data: TextOrBlob::Inline(image.data),
            mime_type: image.mime_type.into(),
            uri: image.uri,
        },
        acp::ContentBlock::Audio(audio) => ContentBlock::Audio {
            data: TextOrBlob::Inline(audio.data),
            mime_type: audio.mime_type.into(),
        },
        acp::ContentBlock::ResourceLink(link) => ContentBlock::ResourceLink {
            uri: link.uri,
            name: link.name,
            mime_type: link.mime_type.map(Into::into),
            title: link.title,
            description: link.description,
            size: link
                .size
                .and_then(|size| u64::try_from(size).ok())
                .map(Count::clamped),
        },
        acp::ContentBlock::Resource(resource) => match resource.resource {
            acp::EmbeddedResourceResource::TextResourceContents(text) => ContentBlock::Resource {
                uri: text.uri,
                mime_type: text.mime_type.map(Into::into),
                text: Some(TextOrBlob::Inline(text.text)),
                blob: None,
            },
            acp::EmbeddedResourceResource::BlobResourceContents(blob) => ContentBlock::Resource {
                uri: blob.uri,
                mime_type: blob.mime_type.map(Into::into),
                text: None,
                blob: Some(TextOrBlob::Inline(blob.blob)),
            },
            other => {
                let whole = serde_json::json!({"type": "resource", "resource": other});
                unsupported("resource".to_owned(), &whole)
            }
        },
        // A block type newer than schema 1.11.0 that the SDK decoded.
        other => {
            let whole = serde_json::to_value(&other).unwrap_or_default();
            let kind = whole
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            unsupported(kind, &whole)
        }
    }
}

/// A block of a type this crate does not know, kept whole as compact JSON text.
pub(crate) fn unsupported(kind: String, whole: &Value) -> ContentBlock {
    ContentBlock::Unsupported {
        kind,
        raw_from: RawFrom::Acp,
        // Writing a `Value` to a string cannot fail: its keys are strings.
        raw: serde_json::to_string(whole).ok().map(TextOrBlob::Inline),
    }
}

/// `text` cut at char boundaries into parts of at most [`MAX_TEXT_CHUNK_BYTES`], in order.
/// Each part is copied out once, so no part keeps the whole text's allocation alive.
pub(crate) fn split_text(text: &str) -> Vec<String> {
    let mut parts = Vec::with_capacity(text.len().div_ceil(MAX_TEXT_CHUNK_BYTES));
    let mut rest = text;
    while !rest.is_empty() {
        let mut cut = MAX_TEXT_CHUNK_BYTES.min(rest.len());
        while !rest.is_char_boundary(cut) {
            cut -= 1;
        }
        let (part, tail) = rest.split_at(cut);
        parts.push(part.to_owned());
        rest = tail;
    }
    parts
}

/// A prompt block as ACP's. `text` and `resource_link` always go; `image`, `audio` and
/// `resource` only when the agent's prompt capabilities allow them. A block that is not inline
/// text, or is `Unsupported`, cannot go.
pub(crate) fn prompt_block(
    block: ContentBlock,
    capabilities: PromptCapabilities,
) -> Result<acp::ContentBlock, Error> {
    Ok(match block {
        ContentBlock::Text { text } => acp::ContentBlock::Text(acp::TextContent::new(text)),
        ContentBlock::ResourceLink {
            uri,
            name,
            mime_type,
            title,
            description,
            size,
        } => {
            let size = size.and_then(|size| i64::try_from(size.get()).ok());
            acp::ContentBlock::ResourceLink(
                acp::ResourceLink::new(name, uri)
                    .mime_type(mime_type.map(String::from))
                    .title(title)
                    .description(description)
                    .size(size),
            )
        }
        ContentBlock::Image {
            data,
            mime_type,
            uri,
        } if capabilities.image => acp::ContentBlock::Image(
            acp::ImageContent::new(inline(data)?, String::from(mime_type)).uri(uri),
        ),
        ContentBlock::Audio { data, mime_type } if capabilities.audio => acp::ContentBlock::Audio(
            acp::AudioContent::new(inline(data)?, String::from(mime_type)),
        ),
        ContentBlock::Resource {
            uri,
            mime_type,
            text,
            blob,
        } if capabilities.embedded_context => {
            let mime_type = mime_type.map(String::from);
            let resource = match (text, blob) {
                (Some(text), None) => acp::EmbeddedResourceResource::TextResourceContents(
                    acp::TextResourceContents::new(inline(text)?, uri).mime_type(mime_type),
                ),
                (None, Some(blob)) => acp::EmbeddedResourceResource::BlobResourceContents(
                    acp::BlobResourceContents::new(inline(blob)?, uri).mime_type(mime_type),
                ),
                (Some(_), Some(_)) | (None, None) => return Err(Error::UnsupportedContent),
            };
            acp::ContentBlock::Resource(acp::EmbeddedResource::new(resource))
        }
        ContentBlock::Image { .. }
        | ContentBlock::Audio { .. }
        | ContentBlock::Resource { .. }
        | ContentBlock::Unsupported { .. } => return Err(Error::UnsupportedContent),
    })
}

/// The text of an inline value; a blob cannot go to the agent from here.
fn inline(value: TextOrBlob) -> Result<String, Error> {
    match value {
        TextOrBlob::Inline(text) => Ok(text),
        TextOrBlob::Blob(_) => Err(Error::UnsupportedContent),
    }
}
