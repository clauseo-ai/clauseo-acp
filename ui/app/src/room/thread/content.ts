// How the thread shows a block that isn't text: an inline raster image through the Markdown renderer's Image, an
// attachment by its name, anything else as one line saying it can't be shown here.
import type { ContentBlock } from "../../transport/protocol.ts";

export type Shown =
  | { kind: "image"; src: string; alt: string }
  | { kind: "attached"; name: string }
  | { kind: "other" };

const RASTER = new Set(["image/png", "image/jpeg", "image/gif", "image/webp"]);

/** The last part of a URI or path, for naming an attachment. */
const nameOf = (uri: string): string => {
  const path = uri.split(/[?#]/)[0] ?? uri;
  return decodeSafely(
    path
      .split(/[\\/]/)
      .filter((p) => p !== "")
      .pop() ?? uri,
  );
};

function decodeSafely(s: string): string {
  try {
    return decodeURIComponent(s);
  } catch {
    return s;
  }
}

export function shownOf(block: ContentBlock): Shown {
  if (block.image !== undefined) {
    const { data, mime_type } = block.image;
    // a blob's bytes stay on the host until read_blob; Image takes only inline data
    if (data.inline !== undefined && RASTER.has(mime_type))
      return { kind: "image", src: `data:${mime_type};base64,${data.inline}`, alt: "" };
    return { kind: "other" };
  }
  if (block.resource_link !== undefined) {
    const link = block.resource_link;
    return { kind: "attached", name: link.title ?? link.name };
  }
  if (block.resource !== undefined) return { kind: "attached", name: nameOf(block.resource.uri) };
  return { kind: "other" };
}
