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
  userMessage,
} from "../../testing/stream-script.ts";
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
