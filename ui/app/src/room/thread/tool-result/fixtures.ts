// U17's fixtures: finished tool calls with no content and a raw output, hand-made from MCP's CallToolResult inside
// codex-acp's {result, error} (no recorded codex-acp sample exists yet: the brief's Backend gaps, 1). The tests, the
// review cells and the gallery read them; each is a ToolCallEntry with `content: []` unless it says otherwise.
import { toolCall } from "../../../testing/stream-script.ts";
import type {
  BlobRef,
  TextOrBlob,
  ToolCallEntry,
  ToolCallStatus,
  ToolKind,
} from "../../../transport/protocol.ts";

let n = 0;
/** A finished call of `kind` with `output` as its raw output (none when undefined). */
export function called(
  output: TextOrBlob | undefined,
  over: Partial<ToolCallEntry> & { status?: ToolCallStatus; kind?: ToolKind } = {},
): ToolCallEntry {
  n += 1;
  const base = toolCall(40 + n, 600 + n, over.status ?? "completed").tool_call!;
  return {
    ...base,
    kind: "other",
    title: "",
    ...(output === undefined ? {} : { raw_output: output }),
    ...over,
  };
}

const inline = (text: string): TextOrBlob => ({ inline: text });
/** codex-acp's opaqueResult, compact as the host keeps it. */
const opaque = (result: unknown, error: unknown = null): TextOrBlob =>
  inline(JSON.stringify({ result, error }));
const text = (t: string) => ({ type: "text", text: t });

export const JUDGMENTS =
  "3 judgments found.\n\nDashrath Rupsingh Rathod v. State of Maharashtra (2014) 9 SCC 129\nMeters and Instruments v. Kanchan Mehta (2018) 1 SCC 560\nMakwana Mangaldas Tulsidas v. State of Gujarat (2020) 4 SCC 1";

/** Inline results: MCP text, a plain object for the JSON form, one past 12 lines, one past 2,000. */
export const MCP_TEXT = inline(
  '{"result":{"content":[{"type":"text","text":"3 judgments found."}]},"error":null}',
);
export const MCP_JUDGMENTS = opaque({ content: [text(JUDGMENTS)] });
export const JSON_RESULT = opaque({
  court: "Supreme Court of India",
  year: 2018,
  bench: ["A. K. Goel", "U. U. Lalit"],
  citation: "(2018) 1 SCC 560",
});
export const LONG_RESULT = opaque(
  Object.fromEntries(
    Array.from({ length: 20 }, (_, i) => [`paragraph_${i + 1}`, `Held at paragraph ${i + 1}.`]),
  ),
);
export const HUGE_RESULT = opaque(Array.from({ length: 2_500 }, (_, i) => i + 1));
/** MCP text with two pictures of a type not drawn (SVG) and one other block, counted, not drawn. */
export const MCP_MIXED = opaque({
  content: [
    text("The page has two figures."),
    { type: "image", data: "PHN2Zy8+", mimeType: "image/svg+xml" },
    { type: "image", data: "PHN2Zy8+", mimeType: "image/svg+xml" },
    { type: "resource", resource: { uri: "file:///x", text: "x" } },
  ],
});

/** A one-pixel PNG, as a screenshot's base64 comes. */
export const PNG =
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";
/** Base64 that reads as a 1×1 PNG's header (its signature and IHDR, the first 24 bytes), padded to `n` characters:
 * not a real picture past its header. */
export const pngOf = (n: number) => PNG.slice(0, 32) + "A".repeat(n - 32);
/** MCP text and a screenshot, small enough to come inline. */
export const MCP_SHOT = opaque({
  content: [text("Took a screenshot."), { type: "image", data: PNG, mimeType: "image/png" }],
});

/** Inline errors: opaque error (with status completed and failed), and MCP's isError. */
export const OPAQUE_ERROR = inline('{"result":null,"error":"connector: out of credit"}');
export const MCP_ERROR = opaque({
  content: [text("Indian Kanoon refused the search: the key was not accepted.")],
  isError: true,
});

/** Inline as a quoted JSON string: a plain string, and an object inside one. */
export const QUOTED = inline('"Done"');
export const QUOTED_OBJECT = inline('"{\\"result\\":\\"ok\\",\\"error\\":null}"');
/** Text that isn't JSON at all. */
export const NOT_JSON = inline("exit 0: 14 files scanned");

/** Empty forms. */
export const NULL = inline("null");
export const NO_BLOCKS = inline('{"result":{"content":[]},"error":null}');

/** Blobs: one read (its text scripted), one past 64 KB, one whose read is refused. */
const blobOf = (hash: string, size_bytes: number): BlobRef => ({
  hash,
  size_bytes,
  preview: '{"result":{"cont',
});
export const BLOB = blobOf("b1".repeat(32), 20_000);
export const BLOB_LARGE = blobOf("b2".repeat(32), 65_537);
export const BLOB_REFUSED = blobOf("b3".repeat(32), 20_000);
/** What reading BLOB gives. */
export const BLOB_TEXT = JSON.stringify({ result: { content: [text(JUDGMENTS)] }, error: null });

/** A full-page screenshot's result, as agent-browser's comes through codex-acp: 1.5 MB, past 64 KB and past one 1 MB
 * read, its image block in the preview. Its base64 is a PNG header and padding, not a real picture. */
export const BLOB_SHOT_TEXT = JSON.stringify({
  result: { content: [{ type: "image", data: pngOf(1_500_000), mimeType: "image/png" }] },
  error: null,
});
export const BLOB_SHOT: BlobRef = {
  hash: "b4".repeat(32),
  size_bytes: BLOB_SHOT_TEXT.length,
  preview: BLOB_SHOT_TEXT.slice(0, 512),
};

const blobFor = (hash: string, text: string): BlobRef => ({
  hash,
  size_bytes: text.length,
  preview: text.slice(0, 512),
});
/** The saved path agent-browser 0.41.1's screenshot puts in its text block (mcp.rs response_text: data.path). */
export const SHOT_PATH =
  "/Users/asha/Library/Application Support/in.clauseo.app/browser/0190aa00-0000-7000-8000-000000000001/screenshots/screenshot-1763270000000.png";
/** agent-browser 0.41.1's screenshot result as it writes it: serde_json without preserve_order sorts the keys, so the
 * image block's `data` comes before its `type`; codex-acp wraps it in {result, error}. */
export const BLOB_SHOT_SORTED_TEXT = JSON.stringify({
  result: {
    content: [
      { text: SHOT_PATH, type: "text" },
      { data: pngOf(200_000), mimeType: "image/png", type: "image" },
    ],
    isError: false,
    structuredContent: { exitCode: 0, stderr: "", stdout: "" },
  },
  error: null,
});
export const BLOB_SHOT_SORTED = blobFor("b6".repeat(32), BLOB_SHOT_SORTED_TEXT);
/** The same with 2 KB of warning and stderr in its text (mcp.rs tool_text), so the preview shows no picture. */
export const BLOB_SHOT_LONG_TEXT = JSON.stringify({
  result: {
    content: [
      {
        text: `${SHOT_PATH}\n\nwarning: ${"w".repeat(1_000)}\n\nstderr:\n${"e".repeat(1_000)}`,
        type: "text",
      },
      { data: pngOf(200_000), mimeType: "image/png", type: "image" },
    ],
    isError: false,
  },
  error: null,
});
export const BLOB_SHOT_LONG = blobFor("b7".repeat(32), BLOB_SHOT_LONG_TEXT);
/** codex-acp's rawInput for agent-browser's screenshot: {server, tool, arguments}. */
export const SHOT_INPUT: TextOrBlob = inline(
  '{"server":"browser","tool":"agent_browser_screenshot","arguments":{"fullPage":true}}',
);
/** Claude's screenshot: claude-agent-acp 0.89.1 sends the MCP result's blocks as content (tool-calls/content.js
 * toAcpContentBlock: text, then {type: "image", data, mimeType}) and its rawOutput in Anthropic's shape; the host keeps
 * the text inline and the image's 1.5 MB of base64 in a blob of its own. */
export const CLAUDE_SHOT_DATA = pngOf(1_500_000);
export const CLAUDE_SHOT_BLOB = blobFor("b8".repeat(32), CLAUDE_SHOT_DATA);
const claudeRaw = JSON.stringify([
  { type: "text", text: SHOT_PATH },
  { type: "image", source: { type: "base64", media_type: "image/png", data: CLAUDE_SHOT_DATA } },
]);
export const CLAUDE_SHOT: Partial<ToolCallEntry> = {
  title: "mcp__browser__agent_browser_screenshot",
  content: [
    { text: { text: inline(SHOT_PATH) } },
    {
      content: { content: { image: { data: { blob: CLAUDE_SHOT_BLOB }, mime_type: "image/png" } } },
    },
  ],
};
export const CLAUDE_SHOT_RAW = blobFor("b9".repeat(32), claudeRaw);
/** A small one, its image inline. */
export const CLAUDE_SHOT_SMALL: Partial<ToolCallEntry> = {
  content: [
    { text: { text: inline("Took a screenshot.") } },
    { content: { content: { image: { data: inline(PNG), mime_type: "image/png" } } } },
  ],
};

/** Hostile outputs: markup, a script link, a bare URL, RTL marks and one 10,000-character line. */
export const HOSTILE: Record<string, TextOrBlob> = {
  img: opaque({ content: [text("<img src=x onerror=alert(1)>")] }),
  script: opaque("<script>alert(1)</script>"),
  link: opaque({ content: [text('<a href="javascript:alert(1)">open</a>')] }),
  url: opaque({ content: [text("See https://indiankanoon.org/doc/1/ for the judgment.")] }),
  rtl: opaque({ content: [text("\u202Eabc\u202C \u200Fمرحبا\u200E done")] }),
  line: opaque({ content: [text("x".repeat(10_000))] }),
  json: opaque({ html: "<b>bold</b>", href: "https://example.com" }),
};
