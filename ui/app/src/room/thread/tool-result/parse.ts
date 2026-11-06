// How a tool call's output is read for its row (U17, Decided here 1), pure: the compact JSON text the host keeps
// (codex-acp's opaqueResult {result, error}, MCP's CallToolResult, or any value) into the text the block shows, its
// face and whether it is an error; and the caps the block draws it by. Nothing here runs before the lawyer presses
// "Show the result": the module rides in the result's lazy chunk.

/** The most text read or shown: 64 KB (an inline output is at most 4 KB, a blob is read up to this). */
export const MAX_TEXT = 65_536;
/** The most lines shown. */
export const MAX_LINES = 2_000;
/** The lines shown before "Show all". */
export const FOLDED_LINES = 12;

/** What the block shows: the text in the thread's face (words) or the code face, whether it is an error, and the
 * MCP blocks it doesn't draw, counted. null: nothing came back. */
export type Read = {
  face: "words" | "code";
  text: string;
  error: boolean;
  pictures: number;
  others: number;
} | null;

type Json = null | boolean | number | string | Json[] | { [key: string]: Json };

const isObject = (v: Json): v is { [key: string]: Json } =>
  typeof v === "object" && v !== null && !Array.isArray(v);

const shown = (face: "words" | "code", text: string, error = false): Read => ({
  face,
  text,
  error,
  pictures: 0,
  others: 0,
});

/** An error's text: a string as itself, anything else as JSON. */
const errorOf = (e: Json): Read =>
  typeof e === "string" ? shown("words", e, true) : shown("code", pretty(e), true);

const pretty = (v: Json): string => JSON.stringify(v, null, 2);

/** Parses JSON text, undefined when it isn't JSON. */
function parsed(text: string): Json | undefined {
  try {
    return JSON.parse(text) as Json;
  } catch {
    return undefined;
  }
}

/** An MCP CallToolResult's text blocks, in order, a blank line between them, its pictures and other blocks counted. */
function mcpOf(result: { [key: string]: Json }, content: Json[]): Read {
  const texts: string[] = [];
  let pictures = 0;
  let others = 0;
  for (const block of content) {
    if (isObject(block) && block["type"] === "text" && typeof block["text"] === "string")
      texts.push(block["text"]);
    else if (isObject(block) && block["type"] === "image") pictures += 1;
    else others += 1;
  }
  const text = texts.join("\n\n");
  if (result["isError"] === true)
    return texts.length > 0 ? shown("words", text, true) : shown("code", pretty(result), true);
  if (content.length === 0) return null;
  return { face: "words", text, error: false, pictures, others };
}

/** Reads a value: an MCP result, a string, nothing, or anything else as pretty JSON. */
function valueOf(v: Json): Read {
  if (isObject(v) && Array.isArray(v["content"])) return mcpOf(v, v["content"]);
  if (typeof v === "string") return v.trim() === "" ? null : shown("words", v);
  if (v === null || (Array.isArray(v) && v.length === 0)) return null;
  if (isObject(v) && Object.keys(v).length === 0) return null;
  return shown("code", pretty(v));
}

/** Reads an output's text: parsed (twice when it was a quoted string holding an object or array), codex-acp's
 * {result, error} unwrapped, then read as a value; text that isn't JSON shows as it is. */
export function readOutput(raw: string): Read {
  const text = raw.length > MAX_TEXT ? raw.slice(0, MAX_TEXT) : raw;
  let v = parsed(text);
  if (v === undefined) return text.trim() === "" ? null : shown("code", text);
  if (typeof v === "string") {
    const inner = parsed(v);
    if (inner !== undefined && typeof inner === "object" && inner !== null) v = inner;
  }
  if (isObject(v)) {
    const keys = Object.keys(v);
    if (keys.length > 0 && keys.every((k) => k === "result" || k === "error")) {
      const error = v["error"];
      if (error !== undefined && error !== null) return errorOf(error);
      return valueOf(v["result"] ?? null);
    }
  }
  return valueOf(v);
}

/** The text as the block draws it: every line up to the caps, the first FOLDED_LINES of it, and whether either cut. */
export type Cut = { all: string; folded: string; long: boolean; capped: boolean };

/** Cuts the shown text at MAX_LINES lines or MAX_TEXT characters, whichever comes first, and folds it to FOLDED_LINES. */
export function cut(text: string): Cut {
  let all = text;
  let capped = false;
  if (all.length > MAX_TEXT) {
    all = all.slice(0, MAX_TEXT);
    capped = true;
  }
  const lines = all.split("\n");
  if (lines.length > MAX_LINES) {
    all = lines.slice(0, MAX_LINES).join("\n");
    capped = true;
  }
  const long = lines.length > FOLDED_LINES;
  return { all, folded: long ? lines.slice(0, FOLDED_LINES).join("\n") : all, long, capped };
}
