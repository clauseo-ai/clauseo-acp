// @vitest-environment happy-dom
// The room's thread over FakeTransport, the rows' side: tool calls, each stop line, the change note and its Undo, the
// limit stop, a notice, images and attachments, a failed turn reopened, older pages, and the clock
// times on questions and answers.
import { act } from "react";
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { brk } from "@clauseo/components/lib/words-breaks.ts";
import { lim } from "@clauseo/components/lib/words-limits.ts";
import { th } from "@clauseo/components/lib/words-thread.ts";
import { t } from "@clauseo/components/lib/words.ts";
import { afterEach, expect, test, vi } from "vitest";
import {
  agentText,
  header,
  reset,
  step,
  STREAM,
  THREAD,
  TURN,
  toolCall,
  upsert,
  userMessage,
  uuid,
} from "../../testing/stream-script.ts";
import type { FileChange, HostError, StepEntry, ViewEntry } from "../../transport/protocol.ts";
import { clockTime, whenOf } from "./clock.ts";
import { defaultOutcome, mountRoom, type Harness } from "./harness.ts";

let h: Harness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
  vi.useRealTimers();
});

const file = (path: string, edited = false): FileChange => ({
  path,
  change: "modified",
  size_bytes: 10,
  may_include_lawyer_edits: edited,
});

const changed = (over: Partial<StepEntry> = {}): ViewEntry =>
  step(9, {
    files: [file("Notice.docx"), file("Reply.docx")],
    checkpoint_after: uuid(1, 9),
    ...over,
  });

test("tool calls: one quiet row each, the title isolated, with its id and status, Failed or Stopped where it ended so", async () => {
  h = await mountRoom();
  const statuses = ["pending", "in_progress", "completed", "failed", "cancelled"] as const;
  h.deliver([reset(statuses.map((s, i) => toolCall(10 + i, 200 + i, s)))]);
  const rows = h.all("tool-call");
  expect(rows.map((r) => r.dataset["status"])).toEqual([...statuses]);
  expect(rows[0]?.dataset["toolCallId"]).toBe(uuid(200, 7));
  expect(rows[0]?.querySelector("bdi")?.textContent).toBe("Read file 200");
  expect(rows.map((r) => r.textContent)).toEqual([
    "Read file 200",
    "Read file 201",
    "Read file 202",
    `Read file 203 · ${th("en", "Failed")}`,
    `Read file 204 · ${th("en", "Stopped")}`,
  ]);
  // an update renders in place
  h.deliver([upsert(toolCall(10, 200, "completed"))]);
  expect(h.all("tool-call")[0]?.dataset["status"]).toBe("completed");
});

test("the answer under a tool call stands the reply's space below it; first in its section, none of its own", async () => {
  h = await mountRoom();
  h.deliver([
    reset([
      userMessage(5, 1, "First"),
      agentText(6, 1, "Straight away"),
      userMessage(7, 2, "Second"),
      toolCall(8, 200, "completed"),
      agentText(9, 2, "After the tool"),
    ]),
  ]);
  const [first, after] = h.all("message-agent");
  // the design's .reply: var(--s4) above the answer, as Tailwind's not-first: variant, so the margin falls only where
  // something stands above it in the answer's section: the meta line, or a tool call
  expect(after?.previousElementSibling?.getAttribute("data-testid")).toBe("tool-call");
  expect(first?.previousElementSibling?.className).toBe("meta meta-live");
  for (const a of [first, after]) {
    expect(a?.classList.contains("not-first:mt-(--s4)")).toBe(true);
    // the design's reply type: 17 px at Clauseo's weight (thread.css)
    expect(a?.classList.contains("reply-text")).toBe(true);
  }
  // a row first in its section takes no space of its own: the gap between sections stands alone
  expect(h.one("tool-call")?.className).toContain("not-first:mt-(--s3)");
});

test("each stop: always a turn-stopped row with its reason, and its line where it has one", async () => {
  h = await mountRoom();
  const stops: [StepEntry["stop"], StepEntry["failure"] | undefined, string][] = [
    ["end_turn", undefined, ""],
    ["at_limit", undefined, ""],
    ["cancelled", undefined, th("en", "You stopped the answer. The answer so far is kept.")],
    ["max_tokens", undefined, th("en", "The answer stopped at its limit, before it finished.")],
    [
      "max_turn_requests",
      undefined,
      th("en", "The answer stopped at its limit, before it finished."),
    ],
    ["refusal", undefined, th("en", "This question was declined, so there is no answer.")],
    ["failed", "agent.launch_failed", errorWords("en", "agent.launch_failed")],
    ["failed", undefined, errorWords("en", "unknown")],
  ];
  h.deliver([
    reset(
      stops.map(([stop, failure], i) => ({
        step: {
          ...step(10 + i).step!,
          turn: uuid(i, 4),
          stop,
          ...(failure === undefined ? {} : { failure }),
        },
      })),
    ),
  ]);
  const rows = h.all("turn-stopped");
  expect(rows.map((r) => r.dataset["stopReason"])).toEqual(stops.map(([s]) => s));
  expect(rows.map((r) => r.textContent)).toEqual(stops.map(([, , line]) => line));
});

test("files changed: the change note names each paper and Undo sends undo for every file", async () => {
  h = await mountRoom();
  h.deliver([reset([changed()])]);
  const note = h.one("files-changed");
  expect(note?.closest('[data-testid="turn-stopped"]')).not.toBeNull();
  expect(h.all("file-changed").map((p) => p.textContent)).toEqual(["Notice.docx", "Reply.docx"]);
  expect(note?.textContent).toContain(th("en", "Clauseo changed {n} papers.", { n: 2 }));
  await h.click(h.one("undo"));
  expect(h.sent("undo_step")).toEqual([
    {
      thread: THREAD,
      turn: TURN,
      files: [
        { path: "Notice.docx", choice: "undo" },
        { path: "Reply.docx", choice: "undo" },
      ],
    },
  ]);
  // the host says when it is undone
  h.deliver([upsert(changed({ undone: true }))]);
  expect(h.one("files-changed")?.textContent).toContain(
    th("en", "You undid Clauseo's changes to {n} papers.", { n: 2 }),
  );
  expect(h.one("undo")).toBeNull();
});

test("one paper reads in the singular", async () => {
  h = await mountRoom();
  h.deliver([reset([changed({ files: [file("Notice.docx")] })])]);
  expect(h.one("files-changed")?.textContent).toContain(th("en", "Clauseo changed 1 paper."));
});

test("a paper edited since asks first: Keep current sends nothing, Undo anyway undoes every file", async () => {
  h = await mountRoom();
  h.deliver([reset([changed({ files: [file("Notice.docx"), file("Reply.docx", true)] })])]);
  const button = (name: string) =>
    [...document.querySelectorAll("button")].find((b) => b.textContent === name);
  // the dialog is its own chunk, loaded on the press (or on reaching Undo): in memory first, as on a real reach
  await act(async () => void (await import("@clauseo/components/clauseo/change-note-ask.tsx")));
  await h.click(h.one("undo"));
  await h.settle();
  expect(document.body.textContent).toContain("Reply.docx");
  await h.click(button("Keep current"));
  expect(h.sent("undo_step")).toEqual([]);
  await h.click(h.one("undo"));
  await h.settle();
  await h.click(button("Undo anyway"));
  expect(h.sent("undo_step")).toHaveLength(1);
  expect(h.sent("undo_step")[0]?.files.map((f) => f.choice)).toEqual(["undo", "undo"]);
});

test("a refused Undo shows its words beside Undo, by code", async () => {
  const refused: HostError = { code: "matter.file_locked", message: "for logs", retryable: true };
  h = await mountRoom({
    dispatch: (command) => {
      if (command.undo_step !== undefined) throw refused;
      return defaultOutcome(command);
    },
  });
  h.deliver([reset([changed()])]);
  await h.click(h.one("undo"));
  const error = h.one("files-changed")?.querySelector<HTMLElement>('[data-testid="error"]');
  expect(error?.dataset["code"]).toBe("matter.file_locked");
  expect(error?.textContent).toBe(errorWords("en", "matter.file_locked"));
  // not on the working line
  expect(h.host.querySelector('.working[data-testid="error"]')).toBeNull();
});

/** Undo waits as DESIGN.md has it: aria-disabled, still in the tab order. */
const waits = () => h!.one("undo")?.getAttribute("aria-disabled") === "true";

test("Undo waits while its undo_step is on its way: one undo however often it is pressed", async () => {
  let release: () => void = () => {};
  const held = new Promise<void>((resolve) => (release = resolve));
  h = await mountRoom({
    dispatch: async (command) => {
      if (command.undo_step !== undefined) await held;
      return defaultOutcome(command);
    },
  });
  h.deliver([reset([changed()])]);
  await h.click(h.one("undo"));
  expect(waits()).toBe(true);
  await h.click(h.one("undo"));
  expect(h.sent("undo_step")).toHaveLength(1);
  await act(async () => release());
  await h.settle();
  expect(waits()).toBe(false);
});

test("Undo waits, faint but focusable, while busy, and a press then sends nothing", async () => {
  h = await mountRoom();
  h.deliver([reset([changed()], header("running"))]);
  expect(waits()).toBe(true);
  expect((h.one("undo") as HTMLButtonElement).disabled).toBe(false);
  await h.click(h.one("undo"));
  expect(h.sent("undo_step")).toEqual([]);
  h.deliver([upsert(header("idle"))]);
  expect(waits()).toBe(false);
});

test("without its checkpoint a step has no Undo, and says why", async () => {
  h = await mountRoom();
  const { checkpoint_after: _, ...bare } = changed().step!;
  h.deliver([reset([{ step: bare }])]);
  expect(h.one("undo")).toBeNull();
  expect(h.one("files-changed")?.textContent).toContain(
    errorWords("en", "matter.checkpoint_failed"),
  );
});

// The thread's clock reads the computer's time zone; this file's times are Asia/Kolkata's whatever zone the run has,
// so the same lines come out under TZ=UTC, TZ=Asia/Kolkata or a CI runner's own zone.
process.env["TZ"] = "Asia/Kolkata";

/** A fixed "now", Monday 2 November 2026 at 3 pm local, so a run that crosses midnight reads the same. */
const NOW = new Date(2026, 10, 2, 15, 0);
const local = (days: number, h: number, m: number) => new Date(2026, 10, 2 + days, h, m).getTime();
const fixNow = () => {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(NOW);
};
const limit = (n: number, reason: string, resets_at?: number): ViewEntry => ({
  limit_stop: {
    order: 10 + n,
    turn: uuid(n, 4),
    reason: reason as never,
    ts: 10_000 + n,
    ...(resets_at === undefined ? {} : { resets_at }),
  },
});
const resetsAt = (row: HTMLElement | undefined) =>
  row?.querySelector<HTMLTimeElement>('[data-testid="limit-resets-at"]') ?? null;

test("the limit stop: the plan's words, its reset as a time today, with the weekday within six days, with the date further off", async () => {
  fixNow();
  h = await mountRoom();
  h.deliver([
    reset([
      limit(1, "plan_window", local(0, 23, 59)),
      limit(2, "workspace_credits", local(2, 9, 5)),
      limit(3, "workspace_spend_cap", local(9, 8, 0)),
    ]),
  ]);
  const [plan, credits, cap] = h.all("limit-stop");
  expect(plan?.textContent).toContain(t("en", "Claude plan limit reached"));
  expect(resetsAt(plan)?.dateTime).toBe(new Date(local(0, 23, 59)).toISOString());
  expect(resetsAt(plan)?.textContent).toBe(lim("en", "Resets {time}", { time: "11:59 pm" }));
  expect(credits?.textContent).toContain(lim("en", "Usage limit reached"));
  // 4 November 2026 is a Wednesday
  expect(resetsAt(credits)?.textContent).toBe(
    lim("en", "Resets {day} {time}", { day: "Wednesday", time: "9:05 am" }),
  );
  expect(resetsAt(cap)?.textContent).toBe(
    lim("en", "Resets {day} {time}", { day: "11 November", time: "8:00 am" }),
  );
});

test("a reset gone by says when it reset, in its time element: two days ago, earlier today, exactly now, ten days back", async () => {
  fixNow();
  h = await mountRoom();
  const gone = [local(-2, 9, 0), local(0, 8, 0), NOW.getTime(), local(-15, 8, 0)];
  h.deliver([reset(gone.map((at, i) => limit(i + 1, "plan_window", at)))]);
  const rows = h.all("limit-stop");
  // 31 October 2026 is a Saturday; ten days and more back reads the date
  const lines = [
    lim("en", "The limit reset on {day} at {time}.", { day: "Saturday", time: "9:00 am" }),
    lim("en", "The limit reset at {time}.", { time: "8:00 am" }),
    lim("en", "The limit reset at {time}.", { time: "3:00 pm" }),
    lim("en", "The limit reset on {day} at {time}.", { day: "18 October", time: "8:00 am" }),
  ];
  expect(rows).toHaveLength(4);
  rows.forEach((row, i) => {
    expect(resetsAt(row)?.dateTime).toBe(new Date(gone[i]!).toISOString());
    expect(resetsAt(row)?.textContent).toBe(lines[i]);
    expect(row.textContent).toBe(t("en", "Claude plan limit reached") + lines[i]);
    expect(row.textContent).not.toContain("Resets");
  });
  expect(lines[3]).toBe("The limit reset on 18 October at 8:00 am.");
});

test("only a used-up plan window is the plan's limit; without a reset the row says its own Reset not given.", async () => {
  fixNow();
  h = await mountRoom();
  h.deliver([
    reset([limit(1, "temporary"), limit(2, "unknown", local(1, 9, 0)), limit(3, "plan_window")]),
  ]);
  const [temporary, unknown, plan] = h.all("limit-stop");
  expect(temporary?.textContent).toBe(
    lim("en", "Usage limit reached") + lim("en", "Reset not given."),
  );
  expect(unknown?.textContent).toContain(lim("en", "Usage limit reached"));
  expect(unknown?.textContent).not.toContain(t("en", "Claude plan limit reached"));
  expect(plan?.textContent).toBe(
    t("en", "Claude plan limit reached") + lim("en", "Reset not given."),
  );
  expect(plan?.textContent).not.toContain("Limit reached.");
});

test("a notice is a quiet line; a marker shows nothing and breaks nothing; a permission shows its prompt", async () => {
  h = await mountRoom();
  h.deliver([
    reset([
      userMessage(5, 1, "Q"),
      {
        notice: {
          order: 6,
          event: uuid(1, 10),
          level: "warning",
          text: "Claude Code was updated.",
          ts: 6000,
        },
      },
      {
        marker: {
          order: 7,
          event: uuid(2, 10),
          kind: { compaction: { status: "started" } } as never,
          ts: 7000,
        },
      },
      {
        permission: {
          order: 8,
          request: uuid(1, 11),
          options: [],
          deliverable: "live",
          state: "pending",
        },
      },
      agentText(9, 1, "Still here"),
    ]),
  ]);
  expect(h.one("notice")?.textContent).toBe("Claude Code was updated.");
  expect(h.one("notice")?.dataset["level"]).toBe("warning");
  expect(h.one("message-agent")?.textContent).toBe("Still here");
  // a permission offering nothing this version knows: its prompt, with no answer to give
  expect(h.one("permission")?.querySelector("button")).toBeNull();
  expect(h.host.querySelectorAll("[data-entry-id]")).toHaveLength(4);
});

test("an inline raster image shows through Image; an attachment is named; anything else is one line", async () => {
  h = await mountRoom();
  const content = (n: number, block: object): ViewEntry => ({
    agent_content: {
      order: 10 + n,
      event: uuid(n, 12),
      turn: TURN,
      message: uuid(1, 6),
      kind: "answer",
      block: block as never,
    },
  });
  h.deliver([
    reset([
      content(1, { image: { data: { inline: "iVBORw0KGgo=" }, mime_type: "image/png" } }),
      content(2, { resource_link: { uri: "file:///x/Lease.pdf", name: "Lease.pdf" } }),
      content(3, { audio: { data: { inline: "AAAA" }, mime_type: "audio/wav" } }),
    ]),
  ]);
  const rows = h.all("agent-content");
  const img = rows[0]?.querySelector("img");
  expect(img?.getAttribute("src")).toBe("data:image/png;base64,iVBORw0KGgo=");
  expect(img?.dataset["clauseo"]).toBe("image");
  // the name isolated (FSI…PDI), so its own direction never reorders the sentence
  expect(rows[1]?.textContent).toBe(
    th("en", "Attached: {name}", { name: "\u2068Lease.pdf\u2069" }),
  );
  expect(rows[2]?.textContent).toBe(th("en", "Something here can't be shown in the thread."));
});

test("reopening a failed turn keeps the question and the answer so far (the brief's promise)", async () => {
  h = await mountRoom();
  h.deliver([
    reset([
      userMessage(5, 1, "Summarise the indemnity clause"),
      agentText(6, 1, "The clause"),
      step(7, { stop: "failed", failure: "agent.exited" }),
    ]),
  ]);
  expect(h.one("message-user")?.textContent).toContain("Summarise the indemnity clause");
  expect(h.one("message-agent")?.textContent).toBe("The clause");
  // agent.exited's step has a true time, so the thread marks where it stopped (U10; stopped-mark.tsx)
  expect(h.one("turn-stopped")?.dataset["failure"]).toBe("agent.exited");
  expect(h.one("turn-stopped")?.textContent).toBe(
    brk("en", "The answer stopped here on {day} at {time}", {
      day: whenOf("en", 7000, Date.now()).day ?? "",
      time: whenOf("en", 7000, Date.now()).time,
    }),
  );
});

test("the latest exchange's question and answer are the thread's two sections; a turn streams only its latest answer", async () => {
  h = await mountRoom();
  h.deliver([
    reset(
      [
        userMessage(5, 1, "First"),
        agentText(6, 1, "One"),
        step(7),
        userMessage(8, 2, "Second"),
        agentText(9, 2, "Two"),
      ],
      header("running"),
    ),
  ]);
  const sections = [...h.host.querySelectorAll("section.step")];
  expect(sections.map((s) => s.className)).toEqual([
    "step step-question",
    "step step-answer",
    "step step-question",
    "step step-answer",
  ]);
  expect(h.host.querySelector(".room-pin-text")?.textContent).toBe("Second");
});

test("older entries page in at the top when the view has more, above the live rows", async () => {
  const older: ViewEntry[] = [
    userMessage(1, 7, "An older question"),
    agentText(2, 7, "An older answer"),
  ];
  h = await mountRoom({
    query: (query) =>
      typeof query === "object" && query.page_entries !== undefined
        ? { entries: older }
        : { entries: [] },
  });
  const window = reset([userMessage(50, 1, "Q"), agentText(51, 1, "A")]);
  window.reset!.view_window.has_older = true;
  h.deliver([window]);
  const scroller = h.host.querySelector<HTMLElement>(".room-thread")!;
  await act(async () => scroller.dispatchEvent(new Event("scroll")));
  await h.settle();
  expect(h.transport.only("query").map((c) => c.query)).toContainEqual({
    page_entries: { stream: STREAM, before_order: 50, limit: 50 },
  });
  const rows = [...h.host.querySelectorAll<HTMLElement>("[data-entry-id]")].map(
    (r) => r.textContent,
  );
  expect(rows[0]).toContain("An older question");
  expect(rows[1]).toBe("An older answer");
  expect(rows.at(-1)).toBe("A");
  // a short page means none older is left: no more asking
  await act(async () => scroller.dispatchEvent(new Event("scroll")));
  await h.settle();
  expect(
    h.transport
      .only("query")
      .filter((c) => typeof c.query === "object" && "page_entries" in c.query),
  ).toHaveLength(1);
});

test("a thread that opens with older entries and can't scroll pages them in at once", async () => {
  const older: ViewEntry[] = [userMessage(1, 7, "An older question")];
  h = await mountRoom({
    query: (query) =>
      typeof query === "object" && query.page_entries !== undefined
        ? { entries: older }
        : { entries: [] },
  });
  const window = reset([userMessage(50, 1, "Q")]);
  window.reset!.view_window.has_older = true;
  h.deliver([window]);
  await h.settle();
  expect(h.transport.only("query").map((c) => c.query)).toContainEqual({
    page_entries: { stream: STREAM, before_order: 50, limit: 50 },
  });
  expect(h.all("message-user")[0]?.textContent).toContain("An older question");
});

test("the log is busy until the first view lands; a streaming answer is busy until it ends", async () => {
  h = await mountRoom();
  const log = () => h!.host.querySelector('[role="log"]');
  expect(log()?.getAttribute("aria-busy")).toBe("true");
  h.deliver([reset([userMessage(5, 1, "Q"), agentText(6, 1, "So far")], header("running"))]);
  expect(log()?.getAttribute("aria-busy")).not.toBe("true");
  expect(h.one("message-agent")?.getAttribute("aria-busy")).toBe("true");
  h.deliver([upsert(step(7)), upsert(header("idle"))]);
  expect(h.one("message-agent")?.hasAttribute("aria-busy")).toBe(false);
});

/** 4:31 pm and 4:33 pm on Monday 2 November 2026 in Asia/Kolkata (UTC+5:30), as the host sends them: ms since the
 * epoch. */
const ASKED = Date.UTC(2026, 10, 2, 11, 1);
const ENDED = Date.UTC(2026, 10, 2, 11, 3);
const sent = (order: number, n: number, text: string, ts: number): ViewEntry => ({
  user_message: { ...userMessage(order, n, text).user_message!, ts },
});

test("a question's Who line is You and the time it was sent; each answer's meta line is Clauseo, with its turn's time once the step is in", async () => {
  fixNow();
  h = await mountRoom();
  h.deliver([
    reset([
      sent(5, 1, "First", ASKED),
      agentText(6, 1, "One"),
      step(7, { ts: ENDED }),
      sent(8, 2, "Second", ENDED),
      agentText(9, 2, "Two so far"),
    ]),
  ]);
  const who = () => h!.all("message-user").map((m) => m.querySelector(".who")?.textContent);
  const metas = () =>
    [...h!.host.querySelectorAll("section.step-answer")].map((a) => a.firstElementChild);
  expect(who()).toEqual([
    `${t("en", "You")}, ${clockTime("en", ASKED)}`,
    `${t("en", "You")}, ${clockTime("en", ENDED)}`,
  ]);
  expect(who()[0]).toBe("You, 4:31 pm");
  expect(metas().map((m) => m?.className)).toEqual(["meta meta-live", "meta meta-live"]);
  // the latest turn runs: its meta line is Clauseo alone until its step comes in
  expect(metas().map((m) => m?.textContent)).toEqual([
    `${t("en", "Clauseo")}, ${clockTime("en", ENDED)}`,
    t("en", "Clauseo"),
  ]);
  h.deliver([upsert(step(10, { turn: uuid(2, 4), ts: ENDED + 60_000 }))]);
  expect(metas()[1]?.textContent).toBe("Clauseo, 4:34 pm");
});
