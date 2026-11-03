// @vitest-environment happy-dom
// The step group in the room over FakeTransport: every run of tool calls and thoughts between two of the agent's texts
// folded into one line with its summary and icons, opened in place by a real button to its rows (each a paper by its
// file name, its folder on hover), a failure kept on the folded line, permission prompts and questions never folded,
// the running group and the working line saying what is being done now, and axe clean in every state, theme and
// language.
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import axe from "axe-core";
import { frame } from "@clauseo/components/lib/words-frame.ts";
import { afterEach, beforeAll, expect, test } from "vitest";
import {
  agentText,
  header,
  permission,
  reset,
  toolCall,
  upsert,
  userMessage,
  uuid,
} from "../../testing/stream-script.ts";
import type {
  ToolCallEntry,
  ToolCallStatus,
  ToolKind,
  ViewEntry,
} from "../../transport/protocol.ts";
import { mountRoom, type Harness } from "./harness.ts";

let h: Harness | null = null;
beforeAll(() => {
  const html = readFileSync(
    join(dirname(fileURLToPath(import.meta.url)), "../../../index.html"),
    "utf8",
  );
  document.title = /<title>([^<]*)<\/title>/.exec(html)?.[1] ?? "";
});
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
});

/** A tool call `n` at `order`, of `kind`, touching `path` inside the matter. */
const call = (
  order: number,
  n: number,
  kind: ToolKind,
  status: ToolCallStatus = "completed",
  over: Partial<ToolCallEntry> = {},
): ViewEntry => ({
  tool_call: { ...toolCall(order, n, status).tool_call!, kind, title: `Tool ${n}`, ...over },
});
const read = (order: number, n: number, path: string, status: ToolCallStatus = "completed") =>
  call(order, n, kind("read"), status, { locations: [{ inside: { path } }] });
const kind = (k: ToolKind) => k;
const search = (order: number, n: number, q: string, status: ToolCallStatus = "completed") =>
  call(order, n, "search", status, { raw_input: { inline: JSON.stringify({ pattern: q }) } });
const elicitation = (order: number): ViewEntry => ({
  elicitation: {
    order,
    elicitation: uuid(1, 9),
    request: { url: { message: "Sign in to Westlaw", url: "https://example.com/login" } },
    state: "pending",
  },
});

const ASKED = userMessage(5, 1, "What does the lease say about notice?");
const groups = () => h!.all("step-group");
const lineOf = (g: HTMLElement | undefined) => g?.querySelector<HTMLButtonElement>("button");

test("everything between two of the agent's texts folds into one group: one line, its summary and icons, no rows", async () => {
  h = await mountRoom();
  h.deliver([
    reset([
      ASKED,
      agentText(6, 1, "Reading the lease first."),
      read(7, 201, "papers/Lease deed.pdf"),
      agentText(8, 2, "Weighing clause nine.", "thought"),
      read(9, 202, "papers/2019/Notice.pdf"),
      search(10, 203, "notice period"),
      agentText(11, 3, "Clause nine needs 30 days' notice."),
      read(12, 204, "papers/Reply.docx"),
    ]),
  ]);
  const [first, second] = groups();
  expect(groups()).toHaveLength(2);
  // the thought folded with the tool calls; the answer's words stand outside
  expect(first?.textContent).toBe("2 papers read. Searched once.");
  expect(second?.textContent).toBe("1 paper read.");
  expect(h.all("message-agent")).toHaveLength(2);
  expect(h.all("tool-call")).toHaveLength(0);
  expect(h.all("thought")).toHaveLength(0);
  // its icons: what was done most, first
  expect(first?.querySelectorAll(".step-group-icons svg")).toHaveLength(2);
  const line = lineOf(first)!;
  expect(line.getAttribute("type")).toBe("button");
  expect(line.getAttribute("aria-expanded")).toBe("false");
});

test("opened in place: its rows, each with its icon, the paper by its file name and its folder on hover", async () => {
  h = await mountRoom();
  h.deliver([
    reset([
      ASKED,
      read(7, 201, "papers/2019/Lease deed.pdf"),
      agentText(8, 2, "Weighing clause nine.", "thought"),
      search(9, 202, "notice period"),
      call(10, 203, "execute", "failed"),
      agentText(11, 3, "Clause nine needs notice."),
    ]),
  ]);
  const line = lineOf(groups()[0])!;
  await h.click(line);
  expect(line.getAttribute("aria-expanded")).toBe("true");
  const rows = h.all("tool-call");
  expect(line.getAttribute("aria-controls")).toBe(h.one("step-group-rows")?.id);
  expect(rows.map((r) => r.textContent)).toEqual([
    "Read Lease deed.pdf.",
    "Searched for 'notice period'.",
    "Running a task. · Failed",
  ]);
  expect(rows.map((r) => r.dataset["kind"])).toEqual(["read", "search", "execute"]);
  expect(rows[0]?.dataset["toolCallId"]).toBe(uuid(201, 7));
  expect(rows[0]?.querySelector("[title]")?.getAttribute("title")).toBe("papers/2019");
  for (const r of rows) expect(r.querySelectorAll("svg")).toHaveLength(1);
  // the thought, in its place among them
  expect(h.one("thought")?.textContent).toBe("Weighing clause nine.");
  // and folded again
  await h.click(line);
  expect(h.all("tool-call")).toHaveLength(0);
  expect(line.getAttribute("aria-expanded")).toBe("false");
});

test("a failure stays visible on the folded line", async () => {
  h = await mountRoom();
  h.deliver([
    reset([
      ASKED,
      read(7, 201, "a/Lease.pdf"),
      call(8, 202, "fetch", "failed"),
      agentText(9, 3, "Done."),
    ]),
  ]);
  const g = groups()[0]!;
  expect(g.dataset["trouble"]).toBe("true");
  expect(h.one("step-group-trouble")?.textContent).toBe("1 of them failed.");
  expect(g.textContent).toBe("1 paper read. Looked online once. 1 of them failed.");
});

test("a permission prompt and a question for the lawyer never fold: each stands between the groups", async () => {
  h = await mountRoom();
  h.deliver([
    reset(
      [
        ASKED,
        read(7, 201, "a/Lease.pdf"),
        call(8, 202, "edit", "pending", { locations: [{ inside: { path: "a/Reply.docx" } }] }),
        permission(9, 202),
        read(10, 203, "a/Notice.pdf"),
        elicitation(11),
        read(12, 204, "a/Deed.pdf"),
      ],
      header("needs_you"),
    ),
  ]);
  expect(groups()).toHaveLength(3);
  const prompt = h.one("permission");
  const question = h.one("elicitation");
  expect(prompt).not.toBeNull();
  expect(question).not.toBeNull();
  for (const g of groups()) {
    expect(g.contains(prompt)).toBe(false);
    expect(g.contains(question)).toBe(false);
  }
  // in order: a group, the prompt, a group, the question, a group
  const order = [
    ...h.host.querySelectorAll(
      '[data-testid="step-group"], [data-testid="permission"], [data-testid="elicitation"]',
    ),
  ].map((e) => e.getAttribute("data-testid"));
  expect(order).toEqual(["step-group", "permission", "step-group", "elicitation", "step-group"]);
});

test("while the agent works, the live group and the working line say what is being done now", async () => {
  h = await mountRoom();
  h.deliver([
    reset(
      [ASKED, read(7, 201, "a/Lease.pdf"), read(8, 202, "papers/Notice.pdf", "in_progress")],
      header("running"),
    ),
  ]);
  const g = () => groups()[0]!;
  expect(g().dataset["live"]).toBe("true");
  expect(g().textContent).toBe("Reading Notice.pdf. 1 done so far.");
  expect(h.one("working")?.textContent).toContain("Reading Notice.pdf.");
  // the next step comes in: the line moves to it
  h.deliver([
    upsert(read(8, 202, "papers/Notice.pdf")),
    upsert(search(9, 203, "notice", "in_progress")),
  ]);
  expect(g().textContent).toBe("Searching for 'notice'. 2 done so far.");
  expect(h.one("working")?.textContent).toContain("Searching for 'notice'.");
  // the answer starts after it: the group settles to its summary, the working line to "Answering."
  h.deliver([upsert(search(9, 203, "notice")), upsert(agentText(10, 3, "Clause nine"))]);
  expect(g().dataset["live"]).toBeUndefined();
  expect(g().textContent).toBe("2 papers read. Searched once.");
  expect(h.one("working")?.textContent).toContain(frame("en", "Answering."));
});

test("in Hindi, the line and the rows in Hindi, the paper's name as filed", async () => {
  h = await mountRoom({ display: { language: "hi" } });
  h.deliver([
    reset([
      ASKED,
      read(7, 201, "papers/Lease deed.pdf"),
      read(8, 202, "papers/Notice.pdf"),
      agentText(9, 3, "ठीक।"),
    ]),
  ]);
  expect(groups()[0]?.textContent).toBe("2 दस्तावेज़ पढ़े गए।");
  await h.click(lineOf(groups()[0]));
  expect(h.all("tool-call")[0]?.textContent).toBe("Lease deed.pdf पढ़ा गया।");
  expect(h.all("tool-call")[0]?.querySelector('[lang="en"]')?.textContent).toBe("Lease deed.pdf");
});

/** Each state a screenshot names: folded, open, live, failed, and a permission among steps. */
const STATES: Record<
  string,
  { entries: ViewEntry[]; status?: "running" | "needs_you"; open?: boolean }
> = {
  folded: {
    entries: [
      ASKED,
      read(7, 201, "a/Lease.pdf"),
      search(8, 202, "notice"),
      agentText(9, 3, "Done."),
    ],
  },
  open: {
    entries: [
      ASKED,
      read(7, 201, "a/Lease.pdf"),
      agentText(8, 2, "Weighing.", "thought"),
      agentText(9, 3, "Done."),
    ],
    open: true,
  },
  live: {
    entries: [ASKED, read(7, 201, "a/Lease.pdf"), read(8, 202, "a/Notice.pdf", "in_progress")],
    status: "running",
  },
  failed: {
    entries: [
      ASKED,
      read(7, 201, "a/Lease.pdf"),
      call(8, 202, "fetch", "failed"),
      agentText(9, 3, "Done."),
    ],
    open: true,
  },
  "a permission among steps": {
    entries: [
      ASKED,
      read(7, 201, "a/Lease.pdf"),
      call(8, 202, "edit", "pending"),
      permission(9, 202),
      read(10, 203, "a/B.pdf"),
    ],
    status: "needs_you",
  },
};

for (const theme of ["light", "dark"] as const) {
  for (const language of ["en", "hi"] as const) {
    test(`axe finds no violation in the step group's states: ${theme}, ${language}`, async () => {
      for (const [name, state] of Object.entries(STATES)) {
        h = await mountRoom({ display: { theme, language } });
        h.deliver([reset(state.entries, header(state.status ?? "idle"))]);
        await h.settle();
        if (state.open === true) await h.click(lineOf(groups()[0]));
        expect(groups().length, name).toBeGreaterThan(0);
        const results = await axe.run(document, { resultTypes: ["violations"] });
        const found = results.violations.map(
          (v) => `${name}: ${v.id} at ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`,
        );
        expect(found).toEqual([]);
        await h.unmount();
        h = null;
      }
    }, 20_000);
  }
}
