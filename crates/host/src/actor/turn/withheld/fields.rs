//! The fields of a tool call event the scrub walks: every string field of the call and of its
//! content, except the agent's `toolCallId` (the host's key for the call) and base64 data (an
//! image's or audio's `data`, a resource's `blob`).

use clauseo_protocol::{
    CompactString, ContentBlock, MatterPath, TextOrBlob, ToolCallContent, ToolCallPatch,
    ToolLocation,
};

use super::Needles;

impl Needles<'_> {
    /// A `tool_call_update`'s patch.
    pub(super) fn patch(&self, patch: &mut ToolCallPatch) {
        if let Some(title) = patch.title.as_mut() {
            self.compact(title);
        }
        if let Some(name) = patch.tool_name.as_mut() {
            self.compact(name);
        }
        for location in patch.locations.iter_mut().flatten() {
            self.location(location);
        }
        if let Some(value) = patch.raw_input.as_mut() {
            self.json(value);
        }
        if let Some(value) = patch.raw_output.as_mut() {
            self.json(value);
        }
        for item in patch.content.iter_mut().flatten() {
            self.content(item);
        }
    }

    /// A `tool_call` that starts after the answer.
    pub(super) fn call(&self, call: &mut clauseo_acp::ToolCall) {
        self.compact(&mut call.title);
        if let Some(name) = call.tool_name.as_mut() {
            self.compact(name);
        }
        for location in &mut call.locations {
            self.location(location);
        }
        if let Some(value) = call.raw_input.as_mut() {
            self.json(value);
        }
        if let Some(value) = call.raw_output.as_mut() {
            self.json(value);
        }
        for item in &mut call.content {
            self.content(item);
        }
    }

    fn compact(&self, text: &mut CompactString) {
        if let Some(new) = self.replace(text) {
            *text = CompactString::from(new);
        }
    }

    fn optional(&self, text: Option<&mut String>) {
        if let Some(text) = text {
            self.text(text);
        }
    }

    fn optional_compact(&self, text: Option<&mut CompactString>) {
        if let Some(text) = text {
            self.compact(text);
        }
    }

    /// A place: a matter path that holds a value is shown as an outside place (it loses its
    /// link) when the scrubbed text is no longer a valid matter path.
    fn location(&self, location: &mut ToolLocation) {
        match location {
            ToolLocation::Inside { path, line } => {
                if let Some(new) = self.replace(path.as_str()) {
                    let line = *line;
                    *location = match MatterPath::new(&new) {
                        Ok(path) => ToolLocation::Inside { path, line },
                        Err(_) => ToolLocation::Outside { display: new },
                    };
                }
            }
            ToolLocation::Outside { display } => self.text(display),
            ToolLocation::Unknown { .. } => {}
        }
    }

    fn inline(&self, value: &mut TextOrBlob) {
        // ACP hands every value over inline; `build.rs` makes the blobs after this.
        if let TextOrBlob::Inline(text) = value {
            self.text(text);
        }
    }

    fn json(&self, value: &mut TextOrBlob) {
        if let TextOrBlob::Inline(text) = value {
            self.json_text(text);
        }
    }

    fn block(&self, block: &mut ContentBlock) {
        match block {
            ContentBlock::Text { text } => self.text(text),
            ContentBlock::Image { mime_type, uri, .. } => {
                self.compact(mime_type);
                self.optional(uri.as_mut());
            }
            ContentBlock::Audio { mime_type, .. } => self.compact(mime_type),
            ContentBlock::ResourceLink {
                uri,
                name,
                mime_type,
                title,
                description,
                ..
            } => {
                self.text(uri);
                self.text(name);
                self.optional_compact(mime_type.as_mut());
                self.optional(title.as_mut());
                self.optional(description.as_mut());
            }
            ContentBlock::Resource {
                uri,
                mime_type,
                text,
                ..
            } => {
                self.text(uri);
                self.optional_compact(mime_type.as_mut());
                if let Some(text) = text.as_mut() {
                    self.inline(text);
                }
            }
            ContentBlock::Unsupported { kind, raw, .. } => {
                self.text(kind);
                if let Some(raw) = raw.as_mut() {
                    self.json(raw);
                }
            }
        }
    }

    pub(super) fn content(&self, item: &mut ToolCallContent) {
        match item {
            ToolCallContent::Text { text } => self.inline(text),
            ToolCallContent::Content { content } => self.block(content),
            ToolCallContent::Diff { path, old, new } => {
                self.location(path);
                if let Some(old) = old.as_mut() {
                    self.inline(old);
                }
                self.inline(new);
            }
            ToolCallContent::Unknown { .. } => {}
        }
    }
}
