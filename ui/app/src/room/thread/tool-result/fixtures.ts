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
/** MCP text with two pictures and one other block, counted, not drawn. */
export const MCP_MIXED = opaque({
  content: [
    text("The page has two figures."),
    { type: "image", data: "iVBORw0KGgo=", mimeType: "image/png" },
    { type: "image", data: "iVBORw0KGgo=", mimeType: "image/png" },
    { type: "resource", resource: { uri: "file:///x", text: "x" } },
  ],
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
