// The stored 403 fixture (#fetch-403): a WebFetch the site refused reads as tried, tagged Failed, in the row and the
// folded line alike, and the line and its tag agree beside a step that finished. Split from steps.test.ts for its size.
import { th } from "@clauseo/components/lib/words-thread.ts";
import { steps } from "@clauseo/components/lib/words-steps.ts";
import { describe, expect, test } from "vitest";
import { toolCall } from "../../testing/stream-script.ts";
import type { ToolCallEntry, ToolKind } from "../../transport/protocol.ts";
import { doneOf, doneSoFar, markOf, restOf, troubleOf } from "./steps.ts";

let n = 0;
/** A tool call of `kind`, completed unless `over` says otherwise. */
function call(kind: ToolKind, over: Partial<ToolCallEntry> = {}): ToolCallEntry {
  n += 1;
  return { ...toolCall(10 + n, 300 + n, "completed").tool_call!, kind, title: "", ...over };
}

describe("a web fetch the site refused reads as tried (httpRefused)", () => {
  // The stored entry of a real WebFetch of https://www.sci.gov.in/ (17 Nov, matter "1198b Sharma v State 2024"): its
  // title, kind, tool name, status and result text as Clauseo stored them; its input's url, the words it names the site by.
  const REFUSED =
    "The server returned HTTP 403 Forbidden.\n\nThe response body was not retrieved. If this URL requires authentication, use an authenticated tool (e.g. `gh` for GitHub, or an MCP-provided fetch tool) instead of WebFetch.";
  const fetched = (text: string, over: Partial<ToolCallEntry> = {}): ToolCallEntry => ({
    ...toolCall(1, 1, "completed").tool_call!,
    title: "Fetch https://www.sci.gov.in/",
    kind: "fetch",
    tool_name: "WebFetch",
    status: "completed",
    raw_input: {
      inline:
        '{"url":"https://www.sci.gov.in/","prompt":"Quote verbatim the first heading on the page (the first h1/h2/etc. heading text in document order). Also list the next few headings verbatim."}',
    },
    content: [{ text: { text: { inline: text } } }],
    ...over,
  });
  const sci = fetched(REFUSED);

  test("the real 403 reads as the tried line, tagged Failed, and counts as failed in the folded line", () => {
    expect(doneOf("en", sci)).toBe("Tried to read sci.gov.in online.");
    expect(markOf("en", sci)).toBe("Failed");
    expect(troubleOf("en", [sci])).toBe("1 failed.");
    expect(restOf("en", [sci])).toEqual({
      kinds: [],
      line: "",
      trouble: "Tried to read sci.gov.in online.",
    });
  });

  test("a 500 reads the same", () => {
    const server = fetched(
      "The server returned HTTP 500 Internal Server Error.\n\nThe response body was not retrieved.",
    );
    expect(doneOf("en", server)).toBe("Tried to read sci.gov.in online.");
    expect(markOf("en", server)).toBe("Failed");
  });

  test("a result that mentions HTTP 403 mid-text is no refusal: still Read, untagged", () => {
    const quoted = fetched(
      "The page's notice reads: The server returned HTTP 403 Forbidden, so the court's mirror was used.",
    );
    expect(doneOf("en", quoted)).toBe("Read sci.gov.in online.");
    expect(markOf("en", quoted)).toBeNull();
    expect(restOf("en", [quoted]).line).toBe("Looked online once.");
  });

  test("another tool with the same text is unchanged", () => {
    const other = fetched(REFUSED, { tool_name: "Bash" });
    expect(doneOf("en", other)).toBe("Read sci.gov.in online.");
    expect(markOf("en", other)).toBeNull();
  });

  test("in Hindi the refused fetch gives the existing tried line, tagged Failed", () => {
    expect(doneOf("hi", sci)).toBe(
      steps("hi", "Tried to read {name} online.", { name: "sci.gov.in" }),
    );
    expect(markOf("hi", sci)).toBe(th("hi", "Failed"));
  });

  test("the line and its tag agree beside a step that finished: one done, one failed", () => {
    expect(restOf("en", [call("read"), sci])).toEqual({
      kinds: ["read"],
      line: "1 paper read.",
      trouble: "1 failed.",
    });
    expect(doneSoFar("en", [sci, call("read")])).toBe("1 done so far.");
  });
});
