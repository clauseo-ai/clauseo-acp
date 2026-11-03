// @vitest-environment happy-dom
// The answer's first step group on its meta line, as the design's chip: "Clauseo, 4:31 pm", then the work's icons, its
// summary and the chevron, its rows opening right under that line. A group between two of the agent's texts stands on
// its own line where it falls, and the meta line then holds the name and the time alone. The running group sits on
// the meta line the same way. The meta line reads only the step's time, so a flush of the answer's text never
// renders it.
import { createElement, Profiler, type ProfilerOnRenderCallback } from "react";
import { afterEach, expect, test, vi } from "vitest";
import {
  agentText,
  append,
  header,
  reset,
  step,
  toolCall,
  upsert,
  userMessage,
} from "../../testing/stream-script.ts";
import type { ToolKind, ViewEntry } from "../../transport/protocol.ts";
import { mountRoom, type Harness } from "./harness.ts";

let metaRenders = 0;
const countMeta: ProfilerOnRenderCallback = () => {
  metaRenders += 1;
};
vi.mock("./answer-meta.tsx", async (load) => {
  const real = await load<typeof import("./answer-meta.tsx")>();
  return {
    ...real,
    AnswerMeta: (props: Parameters<typeof real.AnswerMeta>[0]) =>
      createElement(
        Profiler,
        { id: "meta", onRender: countMeta },
        createElement(real.AnswerMeta, props),
      ),
  };
});

let h: Harness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
  metaRenders = 0;
  document.body.replaceChildren();
});

const ASKED = userMessage(5, 1, "What does the lease say about notice?");
const answer = () => h!.host.querySelector<HTMLElement>("section.step-answer")!;
const metas = () => [...answer().querySelectorAll<HTMLElement>(".meta")];

test("a group before the answer's first words stands on its meta line after the name and the time, and opens right under it", async () => {
  h = await mountRoom();
  h.deliver([
    reset([
      ASKED,
      toolCall(7, 201, "completed"),
      toolCall(8, 202, "completed"),
      agentText(9, 1, "Clause nine needs thirty days' notice."),
      step(10),
    ]),
  ]);
  const group = h.one("step-group")!;
  // the answer opens with the group, whose first part is the one meta line
  expect(answer().firstElementChild).toBe(group);
  expect(metas()).toHaveLength(1);
  const meta = metas()[0]!;
  expect(meta.parentElement).toBe(group);
  expect(meta.className).toBe("meta meta-live");
  const [who, line] = [...meta.children];
  expect(who?.classList.contains("who")).toBe(true);
  expect(who?.textContent).toMatch(/^Clauseo, /);
  expect(line?.tagName).toBe("BUTTON");
  expect(line?.textContent).toBe("2 papers read.");
  // one control: the kind icon and the chevron are inside the button with the words
  expect(line?.querySelector(".step-group-icons svg")).not.toBeNull();
  expect(line?.querySelector(".step-group-chev svg")).not.toBeNull();
  expect(meta.querySelectorAll("svg")).toHaveLength(line!.querySelectorAll("svg").length);
  await h.click(line);
  // opened: the rows under the meta line, then the answer's words
  const rows = h.one("step-group-rows")!;
  expect(meta.nextElementSibling).toBe(rows);
  expect(group.nextElementSibling).toBe(h.one("message-agent"));
});

test("a group between two texts stands on its own line; the meta line holds the name and the time alone", async () => {
  h = await mountRoom();
  h.deliver([
    reset([
      ASKED,
      agentText(6, 1, "Reading the lease first."),
      toolCall(7, 201, "completed"),
      agentText(8, 2, "Clause nine needs thirty days' notice."),
      step(9),
    ]),
  ]);
  expect(metas()).toHaveLength(1);
  const meta = metas()[0]!;
  expect(answer().firstElementChild).toBe(meta);
  expect(meta.querySelector("button")).toBeNull();
  expect(h.one("step-group")!.querySelector(".meta")).toBeNull();
});

test("a thought alone before the words is no chip; once a tool call joins it, the group goes onto the meta line", async () => {
  h = await mountRoom();
  h.deliver([
    reset([ASKED, agentText(6, 1, "Weighing clause nine.", "thought")], header("running")),
  ]);
  expect(h.one("step-group")).toBeNull();
  expect(answer().firstElementChild).toBe(metas()[0]);
  h.deliver([{ upsert: { entry: toolCall(7, 201, "in_progress") } }]);
  expect(metas()).toHaveLength(1);
  expect(metas()[0]!.parentElement).toBe(h.one("step-group"));
});

test("the running group stands on the meta line too, and a flush of the answer's text never renders the meta line", async () => {
  h = await mountRoom();
  h.deliver([
    reset(
      [ASKED, toolCall(7, 201, "completed"), toolCall(8, 202, "in_progress")],
      header("running"),
    ),
  ]);
  const group = h.one("step-group")!;
  expect(group.dataset["live"]).toBe("true");
  expect(metas()[0]!.parentElement).toBe(group);
  expect(metas()[0]!.querySelector("button")?.textContent).toBe("Read file 202. 1 done so far.");
  h.deliver([
    { upsert: { entry: toolCall(8, 202, "completed") } },
    { upsert: { entry: agentText(9, 1, "") } },
  ]);
  const settled = metaRenders;
  for (let i = 0; i < 10; i += 1) h.deliver([append(1, i * 5, "word ")]);
  expect(h.one("message-agent")?.textContent).toContain("word word");
  expect(metaRenders).toBe(settled);
  // the group stayed on the meta line, the answer's words under it
  expect(answer().firstElementChild).toBe(group);
});

/** A finished step of this kind; `aside`: a steer moved it to the background. */
const did = (order: number, id: number, kind: ToolKind, aside = false): ViewEntry => {
  const call = toolCall(order, id, "completed").tool_call!;
  return {
    tool_call: {
      ...call,
      kind,
      ...(aside ? { background: { steer: ASKED.user_message!.message } } : {}),
    },
  };
};

test("in Hindi with three kinds, and in English with a background note, the line stays on the meta line, its whole summary its name and its title", async () => {
  const cases = [
    {
      language: "hi" as const,
      calls: [
        did(7, 201, "read"),
        did(8, 202, "read"),
        did(9, 203, "search"),
        did(10, 204, "edit"),
        did(11, 205, "fetch"),
        did(12, 206, "move"),
      ],
      icons: 3,
    },
    {
      language: "en" as const,
      calls: [did(7, 201, "search"), did(8, 202, "read", true), did(9, 203, "read", true)],
      icons: 2,
    },
  ];
  for (const { language, calls, icons } of cases) {
    h = await mountRoom({ display: { language } });
    h.deliver([reset([ASKED, ...calls, agentText(20, 1, "ठीक।"), step(21)])]);
    const meta = metas()[0]!;
    expect(meta.parentElement).toBe(h.one("step-group"));
    const line = meta.querySelector<HTMLButtonElement>(":scope > button")!;
    // the name and the time, then the line: nothing else, and nothing in a row of its own
    expect(meta.children).toHaveLength(2);
    expect(line.querySelectorAll(".step-group-icons svg")).toHaveLength(icons);
    expect(line.title).not.toBe("");
    expect(line.title).toBe(line.textContent);
    if (language === "en") expect(line.title).toContain("2 of them went on in the background.");
    else expect(line.title).toContain("और 2 काम।");
    await h.unmount();
    h = null;
    document.body.replaceChildren();
  }
});

test("live and folded, the line stands in the one place on the meta line: the same line after the time's kept place, still there as the turn ends", async () => {
  h = await mountRoom();
  h.deliver([
    reset([ASKED, did(7, 201, "read"), toolCall(8, 202, "in_progress")], header("running")),
  ]);
  const meta = metas()[0]!;
  const line = meta.querySelector(":scope > button")!;
  expect(line.textContent).toBe("Read file 202. 1 done so far.");
  // the time's place is kept before the time comes: the widest clock forms, unseen, unheard and in no text
  const held = () =>
    [...meta.querySelectorAll<HTMLElement>(".who-held-as")].map((e) => e.dataset["held"]);
  expect(held()).toEqual([", 10:00 am", ", 10:00 pm"]);
  expect(meta.querySelector(".who")?.textContent).toBe("Clauseo");
  for (const e of meta.querySelectorAll(".who-held-as"))
    expect(e.getAttribute("aria-hidden")).toBe("true");
  // the turn ends: the step finishes, the answer's words come, the turn's step and the idle header
  h.deliver([
    upsert(toolCall(8, 202, "completed")),
    upsert(agentText(9, 1, "Clause nine.")),
    upsert(step(10)),
    upsert(header("idle")),
  ]);
  await h.settle();
  expect(h.one("step-group")!.dataset["live"]).toBeUndefined();
  // the same meta line and the same button, the summary in place of the live words
  expect(metas()[0]).toBe(meta);
  expect(meta.querySelector(":scope > button")).toBe(line);
  expect(line.textContent).toBe("2 papers read.");
  expect(meta.children).toHaveLength(2);
  // the time in the same kept place, the same forms beside it, so the chip after it hasn't moved
  expect(held()).toEqual([", 10:00 am", ", 10:00 pm"]);
  expect(meta.querySelector(".who")?.textContent).toMatch(/^Clauseo, .*\d{1,2}:\d\d [ap]m$/);
  expect(meta.querySelector(".who-held > :not(.who-held-as)")?.textContent).toMatch(/^, /);
});
