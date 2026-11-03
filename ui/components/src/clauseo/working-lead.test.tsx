// @vitest-environment happy-dom
// What leads each line above the field (the icon pass, §8 and §12): the clock for a wait only, the pen in its place
// while Clauseo writes, the break on any error, the offline mark on the room's band, and a choice's question mark in
// its yellow tile.
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { act, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, expect, test } from "vitest";
import { IconBreak, IconChecking, IconOffline, IconQuestion, IconTick } from "../icons/icons.tsx";
import { NoticeCard, RoomBand } from "./limit-note.tsx";
import { RoomScopeProvider } from "./room-scope-provider.tsx";
import { WorkingLine, type WorkingState } from "./working-line.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement | null = null;
let root: Root | null = null;

async function mount(node: ReactNode) {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root?.render(<RoomScopeProvider>{node}</RoomScopeProvider>));
}

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
});

const markup = (node: ReactNode) => renderToStaticMarkup(node);
const CLOCK = markup(<IconChecking size={16} className="working-icon" />);
const BREAK = markup(<IconBreak size={16} className="working-icon" />);

const QUESTION = markup(<IconQuestion size={16} className="working-icon" />);
const STATES: [string, WorkingState, "wait" | "break" | "pen" | "question"][] = [
  [
    "a check",
    { kind: "check", name: "Acme", waitingFor: "the judgments", expected: "16:32" },
    "wait",
  ],
  ["a series", { kind: "series", total: 4, current: null, expected: null }, "wait"],
  ["the queue", { kind: "queued", expected: "16:32" }, "wait"],
  ["the plan's reset", { kind: "limit", resets: "18:10" }, "wait"],
  ["a plain wait", { kind: "wait", words: "Waiting for 'Notice' to finish.", pill: null }, "wait"],
  ["not responding", { kind: "retrying", at: "16:32", lastReply: "16:30" }, "break"],
  ["a stop", { kind: "stopped" }, "break"],
  [
    "an error's code",
    { kind: "wait", words: "That failed.", pill: null, code: "agent.exited" },
    "break",
  ],
  [
    "the lawyer's turn",
    { kind: "wait", words: "Needs your permission.", pill: null, asks: true },
    "question",
  ],
  ["Clauseo writing", { kind: "wait", words: "Answering.", pill: null, working: true }, "pen"],
  [
    "a check while writing",
    { kind: "check", name: "Acme", waitingFor: "it", expected: null, working: true },
    "pen",
  ],
];

test.each(STATES)("the working line on %s leads with its own mark", async (_, state, lead) => {
  await mount(<WorkingLine state={state} />);
  const row = host!.querySelector<HTMLElement>(".working")!;
  expect(row.dataset["lead"]).toBe(lead);
  const icon = row.querySelector(".working-icon")!;
  if (lead === "pen") expect(icon.classList.contains("working-pen")).toBe(true);
  else expect(icon.outerHTML).toBe({ break: BREAK, question: QUESTION, wait: CLOCK }[lead]);
  expect(row.querySelectorAll(".working-icon")).toHaveLength(1);
  // every lead stands in the same slot, its first part, so the words start in one place whatever leads them
  const slot = row.firstElementChild as HTMLElement;
  expect(slot.className).toBe("working-lead");
  expect(slot.dataset["lead"]).toBe(lead);
  expect(slot.children).toHaveLength(1);
  expect(slot.contains(icon)).toBe(true);
});

test("the lead's slot and the pen are one box, fixed by the text size alone, never by the language's line", () => {
  const css = readFileSync(join(dirname(fileURLToPath(import.meta.url)), "live.css"), "utf8");
  const rule = (selector: string) => {
    const at = css.indexOf(`\n  ${selector} {`);
    expect(at, selector).toBeGreaterThan(-1);
    return css.slice(at, css.indexOf("}", at));
  };
  const box = (r: string) => [/width: ([^;]+);/.exec(r)?.[1], /height: ([^;]+);/.exec(r)?.[1]];
  const slot = box(rule(".working-lead"));
  expect(slot).toEqual(["calc(var(--s5) * var(--k) * 110 / 36)", "calc(var(--s5) * var(--k))"]);
  expect(box(rule(".pen-mark"))).toEqual(slot);
  // Hindi's taller line (--lh-small-row) never sizes either
  expect(rule(".working-lead") + rule(".pen-mark")).not.toContain("--lh-");
  // the gap after the lead is the slot's own, not the icon's, so it is the same for every lead
  expect(rule(".working-icon")).not.toContain("margin");
});

test("the room's band leads with the offline mark, not the clock", async () => {
  await mount(
    <RoomBand title="Offline" rows={[{ label: "Still works", text: "Reading papers" }]} />,
  );
  expect(host!.querySelector(".lim-band-icon")?.outerHTML).toBe(
    markup(<IconOffline size={16} className="lim-band-icon" />),
  );
});

test("a choice waiting for the lawyer shows its question mark in the yellow tile; a finished change, the tick", async () => {
  await mount(
    <>
      <NoticeCard waiting icon={<IconQuestion size={16} />} label="Choice" title="Which date?" />
      <NoticeCard
        icon={<IconTick size={16} />}
        label="Saved"
        title="The changes are in the plaint"
      />
    </>,
  );
  const [choice, saved] = [...host!.querySelectorAll<HTMLElement>(".lim-note-icon")];
  expect(choice?.closest("[data-waiting]")).not.toBeNull();
  expect(choice?.innerHTML).toBe(markup(<IconQuestion size={16} />));
  expect(saved?.closest("[data-waiting]")).toBeNull();
  expect(saved?.innerHTML).toBe(markup(<IconTick size={16} />));
});
