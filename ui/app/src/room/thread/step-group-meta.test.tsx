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
import type { ToolKind, TurnId, ViewEntry } from "../../transport/protocol.ts";
import { mountRoom, type Harness } from "./harness.ts";
import { startOf } from "./time-of.ts";
import { stamp } from "./clock.ts";

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

test("in Hindi with three kinds, and in English with a background note, the line stands on the meta line whole: nothing cut, no hover needed, its whole summary its name, the chevron held to its last word", async () => {
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
    // every word is on screen, so there's no hover copy, and the name reads the whole summary
    expect(line.hasAttribute("title")).toBe(false);
    expect(line.querySelector(".step-part-dropped, .step-part-cut")).toBeNull();
    if (language === "en")
      expect(line.textContent).toBe(
        "2 papers read. Searched once. 2 of them went on in the background.",
      );
    else expect(line.textContent).toContain("और 2 काम।");
    // the chevron never stands alone at a line's start: it shares a no-wrap span with the last word
    const chev = line.querySelector(".step-group-chev")!;
    expect(chev.parentElement?.classList.contains("glued")).toBe(true);
    expect(chev.parentElement?.textContent).toBe(language === "en" ? "background." : "काम।");
    await h.unmount();
    h = null;
    document.body.replaceChildren();
  }
});

/** The turn before's question and its step, ended at `ended` ms. */
const T1 = "00000000-0000-4000-8000-0000000000t1" as TurnId;
const turnBefore = (ended: number): ViewEntry[] => [
  { user_message: { ...userMessage(2, 2, "And the deposit?").user_message!, turn: T1 } },
  step(3, { turn: T1, ts: ended }),
];
const lastMeta = () => [...h!.host.querySelectorAll<HTMLElement>(".meta")].at(-1)!;

/** The meta line's time and the start it keeps the place of. */
const timeOf = (meta: HTMLElement) => ({
  who: meta.querySelector(".who")?.textContent,
  held: [...meta.querySelectorAll<HTMLElement>(".who-held-as")].map((e) => e.dataset["held"]),
});

test("live, the meta line shows when the answer started (its question's time, sent to a free agent); at the end the time it ended, in a place kept as wide, the same line and button", async () => {
  h = await mountRoom({ display: { language: "en" } });
  // the question at 5 s (order 5), the turn before ended at 3 s: the agent was free
  h.deliver([
    reset(
      [...turnBefore(3000), ASKED, did(7, 201, "read"), toolCall(8, 202, "in_progress")],
      header("running"),
    ),
  ]);
  const meta = lastMeta();
  const line = meta.querySelector(":scope > button")!;
  expect(line.textContent).toBe("Read file 202. 1 done so far.");
  const asked = stamp("en", ASKED.user_message!.ts, Date.now());
  expect(timeOf(meta)).toEqual({ who: `Clauseo, ${asked}`, held: [`, ${asked}`] });
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
  expect(lastMeta()).toBe(meta);
  expect(meta.querySelector(":scope > button")).toBe(line);
  expect(line.textContent).toBe("2 papers read.");
  expect(meta.children).toHaveLength(2);
  // the time it ended, the start still keeping the place's width
  const ended = stamp("en", 10_000, Date.now());
  expect(timeOf(meta)).toEqual({ who: `Clauseo, ${ended}`, held: [`, ${asked}`] });
});

test("a question that waited shows no time while live: behind the turn before, or in the matter's queue", async () => {
  // the turn before ended at 9 s, after the question was sent at 5 s: it waited in the thread's queue
  h = await mountRoom();
  h.deliver([
    reset([...turnBefore(9000), ASKED, toolCall(8, 202, "in_progress")], header("running")),
  ]);
  expect(timeOf(lastMeta())).toEqual({ who: "Clauseo", held: [] });
  await h.unmount();
  document.body.replaceChildren();
  // waiting behind another conversation of the matter: a queued turn has only its question
  h = await mountRoom();
  const queued: ViewEntry = {
    header: { ...header().header!, status: { queued: { behind: "thread:other" as never } } },
  };
  h.deliver([reset([ASKED], queued)]);
  await h.settle();
  expect(timeOf(lastMeta())).toEqual({ who: "Clauseo", held: [] });
  // the other conversation finishes and this turn runs, its first step in: the meta line moves onto the group, and
  // the turn still waited, so still no time while live
  h.deliver([upsert(header("running")), upsert(toolCall(8, 202, "in_progress"))]);
  await h.settle();
  expect(lastMeta().parentElement).toBe(h.one("step-group"));
  expect(timeOf(lastMeta())).toEqual({ who: "Clauseo", held: [] });
  // its words come after the step: the meta line stands alone again, still no time
  h.deliver([upsert(toolCall(8, 202, "completed")), upsert(agentText(9, 1, "Clause nine."))]);
  await h.settle();
  expect(timeOf(lastMeta())).toEqual({ who: "Clauseo", held: [] });
  // at the end, the time it ended
  h.deliver([upsert(step(10)), upsert(header("idle"))]);
  await h.settle();
  expect(timeOf(lastMeta()).who).toBe(`Clauseo, ${stamp("en", 10_000, Date.now())}`);
});

test("the start time is honest or none: startOf", () => {
  expect(startOf(5000, 3000, false)).toBe(5000);
  expect(startOf(5000, null, false)).toBe(5000);
  expect(startOf(5000, 6000, false)).toBeNull();
  expect(startOf(5000, 3000, true)).toBeNull();
  expect(startOf(null, 3000, false)).toBeNull();
  // a question with no time of its own gives none
  expect(startOf(0, null, false)).toBeNull();
  expect(startOf(-1, null, false)).toBeNull();
});
