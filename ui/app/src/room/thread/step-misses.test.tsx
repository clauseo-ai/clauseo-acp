// @vitest-environment happy-dom
// A page that couldn't open and a step Claude's safety check held back are never "failed" (Windows tester item C):
// each reason for both agents, the folded line's count, and "failed" kept for a paper not read, an edit not saved and
// a command that broke. Steps shaped as the agents send them (testing/step-lines.ts).
import { act } from "react";
import { afterEach, expect, test } from "vitest";
import { agentText, reset, userMessage } from "../../testing/stream-script.ts";
import { AGENTS, commandOf, fetchOf, heldOf, readOf, stepCall } from "../../testing/step-lines.ts";
import type { ToolCallEntry } from "../../transport/protocol.ts";
import { mountRoom, type Harness } from "./harness.ts";
import { missOf } from "./step-misses.ts";
import { doneOf, lineOf, markOf, troubleOf } from "./steps.ts";

let h: Harness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
});

const URL = "https://www.livelaw.in/high-court/";

/** Each error as the fetch reports it, its reason, and its row. */
const ERRORS: [said: string, miss: string, row: string][] = [
  ["Request failed with status code 403", "refused", "Couldn't open livelaw.in: the site refused."],
  ["Request failed with status code 401", "refused", "Couldn't open livelaw.in: the site refused."],
  ["Request failed with status code 404", "not_found", "Couldn't open livelaw.in: not found."],
  [
    "getaddrinfo ENOTFOUND www.livelaw.in",
    "unreachable",
    "Couldn't open livelaw.in: couldn't reach the site.",
  ],
  [
    "connect ECONNREFUSED 104.21.3.4:443",
    "unreachable",
    "Couldn't open livelaw.in: couldn't reach the site.",
  ],
  [
    "maxContentLength size of 10485760 exceeded",
    "too_large",
    "Couldn't open livelaw.in: too large.",
  ],
  ["timeout of 60000ms exceeded", "slow", "Couldn't open livelaw.in: the site took too long."],
  ["Something else went wrong", "unknown", "Couldn't open livelaw.in."],
];

test.each(AGENTS.flatMap((agent) => ERRORS.map((e) => [agent, ...e] as const)))(
  "under %s, a fetch that failed with %j couldn't open its page (%s), untagged",
  (agent, said, miss, row) => {
    const c = fetchOf(agent, URL, "failed", said);
    expect(missOf(c)).toBe(miss);
    expect(doneOf("en", c)).toBe(row);
    expect(markOf("en", c)).toBeNull();
    expect(troubleOf("en", [c])).toBe("");
  },
);

test("Claude's completed WebFetch with the site's 403 or 404 is a page that couldn't open", () => {
  const forbidden = fetchOf("Claude", URL, "completed", "The server returned HTTP 403 Forbidden.");
  const gone = fetchOf("Claude", URL, "completed", "The server returned HTTP 404 Not Found.");
  expect(doneOf("en", forbidden)).toBe("Couldn't open livelaw.in: the site refused.");
  expect(doneOf("en", gone)).toBe("Couldn't open livelaw.in: not found.");
  // a page that opened is no miss
  expect(missOf(fetchOf("Claude", URL, "completed", "The order is dated 12 March."))).toBeNull();
});

test("the tester's card: nine pages that couldn't open and two held back, none of them failed", () => {
  const calls: ToolCallEntry[] = [
    readOf("Claude", "Plaint.docx"),
    ...Array.from({ length: 4 }, () =>
      fetchOf("Claude", URL, "completed", "The server returned HTTP 403 Forbidden."),
    ),
    fetchOf("Claude", URL, "failed", "Request failed with status code 404"),
    fetchOf("Claude", URL, "failed", "getaddrinfo ENOTFOUND www.livelaw.in"),
    fetchOf("Claude", URL, "failed", "connect ECONNREFUSED 104.21.3.4:443"),
    fetchOf("Claude", URL, "failed", "maxContentLength size of 10485760 exceeded"),
    fetchOf("Claude", URL, "failed", "timeout of 60000ms exceeded"),
    heldOf(),
    heldOf(),
  ];
  const line = lineOf(
    "en",
    calls.map((tool_call) => ({ tool_call })),
    false,
  );
  expect(line.line).toBe("Read 1 paper.");
  expect(line.trouble).toBe("");
  expect(line.more).toBe("Couldn't open 9 pages. 2 held back by Claude's safety check.");
  expect(markOf("en", heldOf())).toBe("Held back by Claude's safety check");
});

test("only setbacks: the line says them, with no break", () => {
  const line = lineOf("en", [{ tool_call: heldOf() }], false);
  expect(line.line).toBe("1 held back by Claude's safety check.");
  expect(line.trouble).toBe("");
  expect(line.more).toBe("");
});

test.each(AGENTS)(
  "under %s, failed stays for a paper not read, an edit not saved and a command that broke",
  (agent) => {
    const paper = readOf(agent, "Plaint.docx", "failed");
    const edit = stepCall("edit", "Edit Reply.docx", "failed", {
      locations: [{ inside: { path: "Reply.docx" } }],
    });
    const command = commandOf(agent, "pdfinfo Plaint.pdf", null, "failed");
    for (const c of [paper, edit, command]) {
      expect(missOf(c)).toBeNull();
      expect(markOf("en", c)).toBe("Failed");
    }
    expect(troubleOf("en", [readOf(agent, "a.pdf"), paper, edit, command])).toBe("3 failed.");
  },
);

test("in the room, the card counts the pages that couldn't open and lists each with its reason", async () => {
  h = await mountRoom();
  const calls = [
    readOf("Claude", "Plaint.docx"),
    fetchOf("Claude", URL, "completed", "The server returned HTTP 403 Forbidden."),
    fetchOf(
      "Claude",
      "https://www.sci.gov.in/x.pdf",
      "failed",
      "maxContentLength size of 10485760 exceeded",
    ),
  ].map((tool_call) => ({ tool_call }));
  h.deliver([
    reset([
      userMessage(5, 1, "Find the Delhi High Court's latest order on the possession notice."),
      ...calls,
      agentText(30, 1, "The order is dated 12 March 2024."),
    ]),
  ]);
  await h.settle();
  const group = h.one("step-group")!;
  expect(group.textContent).toContain("Couldn't open 2 pages.");
  expect(group.textContent).not.toMatch(/failed/i);
  await act(async () => group.querySelector<HTMLButtonElement>("button")!.click());
  await h.settle();
  const text = h.one("step-group")!.textContent ?? "";
  expect(text).toContain("Couldn't open livelaw.in: the site refused.");
  expect(text).toContain("Couldn't open sci.gov.in: too large.");
  expect(text).not.toMatch(/Failed|maxContentLength|HTTP 403/);
});
