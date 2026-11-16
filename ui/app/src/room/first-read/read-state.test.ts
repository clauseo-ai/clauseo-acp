// The first read's line (read-state.ts): the file a read names, from codex's quoted shell reads too, and the line's
// words, which never carry a quote the agent's command had round its path (D-019); and the working line's words for the
// same read (step-names.ts), whose quoted path may hold a space.
import { expect, test } from "vitest";
import type { ToolCallEntry } from "../../transport/protocol.ts";
import { doingOf, titleOf } from "../thread/step-names.ts";
import { fileOf, readLine, readLineParts } from "./read-state.ts";

const read = (title: string, locations: ToolCallEntry["locations"] = []): ToolCallEntry => ({
  order: 3,
  tool_call: "00000000-0000-7000-8000-0000000000a7",
  turn: "00000000-0000-7000-8000-000000000004",
  started_at: 3000,
  title,
  kind: "read",
  status: "in_progress",
  locations,
  content: [],
});

/** codex-acp's read of the agent's notes, outside the matter, as stage C's session log has it. */
const CODEX_CAT =
  "cat '/Users/someone/Library/Caches/clauseo/agent-state/01a1/notes/matter-brief.md'";

test("a read inside the matter names its file from the location", () => {
  expect(fileOf(read("Read", [{ inside: { path: "papers/Board minutes.docx" } }]))).toBe(
    "Board minutes.docx",
  );
});

test("codex's quoted cat of a file outside the matter names the file, with no stray quote", () => {
  expect(fileOf(read(CODEX_CAT))).toBe("matter-brief.md");
  expect(fileOf(read(`Read ${CODEX_CAT.slice(4)}`))).toBe("matter-brief.md");
  expect(fileOf(read('cat "/tmp/notes/matter-brief.md"'))).toBe("matter-brief.md");
});

test("a quoted path with a space in a folder names the file, in the first read's line and the working line", () => {
  const spaced = read(
    "cat '/Users/someone/Library/Application Support/Clauseo/agent-state/01a1/notes/matter-brief.md'",
  );
  expect(fileOf(spaced)).toBe("matter-brief.md");
  expect(doingOf("en", spaced)).toBe("Reading matter-brief.md.");
  expect(titleOf(spaced.title)).toBe("cat 'matter-brief.md'");
  // a bare path still runs on past a space to its paper's extension
  expect(fileOf(read("Read /Users/someone/matter/papers/Form DIR-12.pdf"))).toBe("Form DIR-12.pdf");
});

test("a title with no path names no file, so the line counts the files alone", () => {
  expect(fileOf(read("Read"))).toBeNull();
  expect(readLine("en", { kind: "reading", title: fileOf(read("Read")) }, 4)).toBe(
    "Reading 4 files.",
  );
});

test("the reading line, in English and Hindi, has no quote in it", () => {
  const state = { kind: "reading", title: fileOf(read(CODEX_CAT)) } as const;
  const en = readLine("en", state, 4);
  expect(en).toBe("Reading 4 files. Now \u2068matter-brief.md\u2069.");
  expect(en).not.toMatch(/['"`’]/);
  const hi = readLine("hi", state, 4);
  expect(hi).toContain("\u2068matter-brief.md\u2069।");
  expect(hi).not.toMatch(/['"`’]/);
});

test("the line's parts cut the file's name out at its isolates, for the one-line layout", () => {
  const state = { kind: "reading", title: "matter-brief.md" } as const;
  expect(readLineParts("en", state, 4)).toEqual({
    before: "Reading 4 files. Now ",
    name: "matter-brief.md",
    after: ".",
  });
  const hi = readLineParts("hi", state, 4);
  expect(hi.name).toBe("matter-brief.md");
  expect(hi.after).toBe("।");
  expect(readLineParts("en", { kind: "reading", title: null }, 4)).toEqual({
    before: "Reading 4 files.",
    name: null,
    after: "",
  });
  expect(readLineParts("en", { kind: "read" }, 4).name).toBeNull();
});
