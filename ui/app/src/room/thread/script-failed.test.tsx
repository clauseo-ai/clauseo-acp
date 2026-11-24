// @vitest-environment happy-dom
// A code mode script that failed (stage B's B4 notice, kind "script_failed", level info): one quiet line inside the
// step group, in the app's words, between the steps around it; never counted, never splitting the group, never
// Claude's raw sentence, one line however often a script failed. Any other notice stands alone as it does today.
import { afterEach, expect, test } from "vitest";
import { agentText, reset, userMessage } from "../../testing/stream-script.ts";
import { noticeOf, readOf, SCRIPT_RAW, searchOf } from "../../testing/step-lines.ts";
import type { ToolCallEntry, ViewEntry } from "../../transport/protocol.ts";
import { mountRoom, type Harness } from "./harness.ts";

let h: Harness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
});

const LINE = "A script didn't run; Claude tried another way.";
const FAILED = { kind: "script_failed", level: "info", text: SCRIPT_RAW } as const;
const at = (c: ToolCallEntry, order: number): ViewEntry => ({ tool_call: { ...c, order } });

async function answer(middle: ViewEntry[]) {
  h = await mountRoom();
  h.deliver([
    reset([
      userMessage(5, 1, "Read the plaint and the loan agreement and tell me the notice date."),
      at(readOf("Claude", "Plaint.docx"), 40),
      at(readOf("Claude", "Written statement.pdf"), 41),
      ...middle,
      at(readOf("Claude", "Loan agreement.pdf"), 43),
      at(searchOf("Claude", "notice period"), 44),
      agentText(90, 1, "The notice is dated 4 January 2024."),
    ]),
  ]);
  await h.settle();
}

test("the line sits inside the one group, between the steps, and is not counted", async () => {
  await answer([noticeOf(42, FAILED)]);
  const groups = h!.all("step-group");
  expect(groups).toHaveLength(1);
  const line = groups[0]!.querySelector("button")!.textContent ?? "";
  expect(line).toContain("Read 3 papers, searched the papers.");
  expect(line).not.toMatch(/failed|script|setback/i);
  await h!.click(groups[0]!.querySelector("button"));
  const rows = [...h!.one("step-group")!.querySelectorAll("li")].map((li) => li.textContent ?? "");
  expect(rows).toHaveLength(5);
  expect(rows[2]).toBe(LINE);
  expect(rows[1]).toContain("Read Written statement.pdf.");
  expect(rows[3]).toContain("Read Loan agreement.pdf.");
});

test("the raw sentence never renders, and several failures read as one line", async () => {
  await answer([noticeOf(42, FAILED), noticeOf(45, FAILED)]);
  await h!.click(h!.one("step-group")!.querySelector("button"));
  const text = document.body.textContent ?? "";
  expect(text).not.toContain("Code that Claude ran failed");
  expect(text.split(LINE)).toHaveLength(2);
  expect(h!.all("script-failed")).toHaveLength(1);
});

test("a plain warning notice still stands alone and parts the run, as today", async () => {
  await answer([noticeOf(42)]);
  expect(h!.all("step-group")).toHaveLength(2);
  expect(document.body.textContent).not.toContain(LINE);
});

test("an info notice of another kind still renders as today, standing alone", async () => {
  await answer([noticeOf(42, { kind: "context_not_restored", level: "info", text: "fallback" })]);
  expect(h!.all("step-group")).toHaveLength(2);
  expect(h!.all("notice")).toHaveLength(1);
  expect(h!.one("notice")!.textContent).not.toBe(LINE);
});
