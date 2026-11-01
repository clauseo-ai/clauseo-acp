// @vitest-environment happy-dom
// The room's thread over FakeTransport, the rows' side: tool calls, each stop line, the change note and its Undo, the
// limit stop, a notice, images and attachments, a failed turn reopened, older pages, and H17's clock times.
import { act } from "react";
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { lim } from "@clauseo/components/lib/words-limits.ts";
import { th } from "@clauseo/components/lib/words-thread.ts";
import { t } from "@clauseo/components/lib/words.ts";
import { afterEach, expect, test } from "vitest";
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
import { withTime } from "../host-next.ts";
import { clockTime } from "./clock.ts";
import { defaultOutcome, mountRoom, type Harness } from "./harness.ts";

let h: Harness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
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
  await h.click(h.one("undo"));
  expect(document.body.textContent).toContain("Reply.docx");
  await h.click(button("Keep current"));
  expect(h.sent("undo_step")).toEqual([]);
  await h.click(h.one("undo"));
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
  const undo = () => h!.one("undo") as HTMLButtonElement;
  await h.click(undo());
  expect(undo().disabled).toBe(true);
  await h.click(undo());
  expect(h.sent("undo_step")).toHaveLength(1);
  await act(async () => release());
  await h.settle();
  expect(undo().disabled).toBe(false);
});

test("Undo is disabled while busy", async () => {
  h = await mountRoom();
  h.deliver([reset([changed()], header("running"))]);
  expect((h.one("undo") as HTMLButtonElement).disabled).toBe(true);
  h.deliver([upsert(header("idle"))]);
  expect((h.one("undo") as HTMLButtonElement).disabled).toBe(false);
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

test("the limit stop: the plan's words, its reset as a time today and with the weekday on another day", async () => {
  h = await mountRoom();
  const now = new Date();
  const today = new Date(now.getFullYear(), now.getMonth(), now.getDate(), 23, 59).getTime();
  const later = new Date(now.getFullYear(), now.getMonth(), now.getDate() + 2, 9, 5).getTime();
  const limit = (n: number, reason: string, resets_at?: number): ViewEntry => ({
    limit_stop: {
      order: 10 + n,
      turn: uuid(n, 4),
      reason: reason as never,
      ...(resets_at === undefined ? {} : { resets_at }),
    },
  });
  h.deliver([
    reset([
      limit(1, "plan_window", today),
      limit(2, "workspace_credits", later),
      limit(3, "temporary"),
    ]),
  ]);
  const [plan, credits, none] = h.all("limit-stop");
  expect(plan?.textContent).toContain(t("en", "Claude plan limit reached"));
  const at = plan?.querySelector<HTMLTimeElement>('[data-testid="limit-resets-at"]');
  expect(at?.dateTime).toBe(new Date(today).toISOString());
  expect(at?.textContent).toBe(lim("en", "Resets {time}", { time: "11:59 pm" }));
  expect(credits?.textContent).toContain(lim("en", "Usage limit reached"));
  const day = new Intl.DateTimeFormat("en-GB", { weekday: "long" }).format(new Date(later));
  expect(credits?.querySelector('[data-testid="limit-resets-at"]')?.textContent).toBe(
    lim("en", "Resets {day} {time}", { day, time: "9:05 am" }),
  );
  expect(none?.querySelector('[data-testid="limit-resets-at"]')).toBeNull();
  expect(none?.textContent).toContain(lim("en", "Limit reached. Reset not given."));
});

test("a notice is a quiet line; markers, versions, permissions and elicitations show nothing and break nothing", async () => {
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
        },
      },
      {
        marker: {
          order: 7,
          event: uuid(2, 10),
          kind: { compaction: { status: "started" } } as never,
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
  expect(h.host.querySelectorAll("[data-entry-id]")).toHaveLength(3);
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
  expect(rows[1]?.textContent).toBe(th("en", "Attached: {name}", { name: "Lease.pdf" }));
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
  expect(h.one("turn-stopped")?.textContent).toBe(
    th("en", "The answer stopped. Your question and the answer so far are kept."),
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

test('H17: a question shows its time when the host gives one, and "You" alone when it doesn\'t', async () => {
  h = await mountRoom();
  const now = new Date();
  const at = new Date(now.getFullYear(), now.getMonth(), now.getDate(), 0, 1).getTime();
  h.deliver([reset([withTime(userMessage(5, 1, "Timed"), at), userMessage(8, 2, "Untimed")])]);
  const [timed, untimed] = h.all("message-user").map((m) => m.querySelector(".who")?.textContent);
  expect(timed).toBe(`${t("en", "You")}, ${clockTime("en", at)}`);
  expect(timed).toBe("You, 12:01 am");
  expect(untimed).toBe(t("en", "You"));
});
