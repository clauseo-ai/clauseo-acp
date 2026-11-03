// @vitest-environment happy-dom
// What leads each line above the field (the icon pass, §8 and §12): the clock for a wait only, the empty slot while
// Clauseo writes and the pen after the words, the break on any error, the offline mark on the room's band, and a choice's question mark in
// its yellow tile.
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { act, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, expect, test } from "vitest";
import {
  IconBreak,
  IconChecking,
  IconNeedsPermission,
  IconOffline,
  IconQuestion,
  IconTick,
} from "../icons/icons.tsx";
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
const PERMISSION = markup(<IconNeedsPermission size={16} className="working-icon" />);
const STATES: [string, WorkingState, "wait" | "break" | "pen" | "permission" | "question"][] = [
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
    "a permission waiting on the lawyer",
    { kind: "wait", words: "Needs your permission.", pill: null, asks: "permission" },
    "permission",
  ],
  [
    "a question waiting on the lawyer",
    { kind: "wait", words: "A question for you.", pill: null, asks: "question" },
    "question",
  ],
  [
    "a permission while the pen would write",
    {
      kind: "wait",
      words: "Needs your permission.",
      pill: null,
      asks: "permission",
      working: true,
    },
    "permission",
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
  else
    expect(icon.outerHTML).toBe(
      { break: BREAK, permission: PERMISSION, question: QUESTION, wait: CLOCK }[lead],
    );
  expect(row.querySelectorAll(".working-icon")).toHaveLength(1);
  // every lead stands in the same slot, its first part, so the words start in one place whatever leads them; the
  // pen leaves the slot empty and writes after the last word
  const slot = row.firstElementChild as HTMLElement;
  expect(slot.className).toBe("working-lead");
  expect(slot.dataset["lead"]).toBe(lead);
  const words = row.querySelector(".working-text > span")!;
  if (lead === "pen") {
    expect(slot.children).toHaveLength(0);
    // held to the last word, in one no-wrap span, the words' last part
    expect(words.lastElementChild).toBe(icon.parentElement);
    expect(icon.parentElement?.className).toBe("glued");
    expect(icon.nextSibling).toBeNull();
  } else {
    expect(slot.children).toHaveLength(1);
    expect(slot.contains(icon)).toBe(true);
    expect(words.querySelector(".working-icon")).toBeNull();
  }
});

test("the pen never stands alone on a line: it shares a no-wrap span with the sentence's last word, in either language, a long name's too, and the words read whole", async () => {
  const name = "Written statement of respondent no. 2 to the complaint, final copy (12).pdf";
  expect(name).toHaveLength(75);
  for (const [words, last] of [
    [`Reading ${name}.`, "(12).pdf."],
    [`${name} पढ़ा जा रहा है।`, "है।"],
    ["Answering.", "Answering."],
    // a last word with no spaces keeps only its last letters with the pen, so the rest can break
    ["Reading Written_statement_of_respondent_no_2_final.pdf.", "_final.pdf.".slice(-6)],
  ] as const) {
    await mount(<WorkingLine state={{ kind: "wait", words, pill: null, working: true }} />);
    const text = host!.querySelector(".working-text > span")!;
    const pen = text.querySelector(".working-pen")!;
    expect(pen.parentElement?.className).toBe("glued");
    expect(pen.parentElement?.textContent).toBe(last);
    expect(text.textContent).toBe(words);
    await act(async () => root?.unmount());
    host?.remove();
    host = null;
    root = null;
  }
  const css = readFileSync(join(dirname(fileURLToPath(import.meta.url)), "live.css"), "utf8");
  expect(css).toMatch(/\.working-text \{[^}]*overflow-wrap: anywhere;/);
});

test("every lead is 16 px, the pen's empty slot too, so the words start at the prototype's x 48 in every state; the pen's box is fixed and the same in either language", () => {
  const css = readFileSync(join(dirname(fileURLToPath(import.meta.url)), "live.css"), "utf8");
  const rule = (selector: string) => {
    const at = css.indexOf(`\n  ${selector} {`);
    expect(at, selector).toBeGreaterThan(-1);
    return css.slice(at, css.indexOf("}", at));
  };
  const px: Record<string, number> = { "var(--s2)": 8, "var(--s4)": 16 };
  // the prototype's working line: the wait icon at x 16, the words at x 48, over the field's own
  const row = rule(".working");
  const padStart = /padding: 0 var\(--s1-5\) 0 (var\(--s4\));/.exec(row)?.[1];
  const gap = /column-gap: (var\(--s2\));/.exec(row)?.[1];
  const lead = rule(".working-lead");
  const after = /margin-right: (var\(--s2\));/.exec(lead)?.[1];
  expect(lead).not.toMatch(/width|height/);
  expect(px[padStart!]! + 16 + px[after!]! + px[gap!]!).toBe(48);
  // the pen's empty slot: an icon's 16 px
  const box = (r: string) => [/width: ([^;]+);/.exec(r)?.[1], /height: ([^;]+);/.exec(r)?.[1]];
  expect(box(rule('.working-lead[data-lead="pen"]'))).toEqual(["var(--s4)", "var(--s4)"]);
  // the pen's box: fixed by the text size alone, never by Hindi's taller line
  expect(box(rule(".pen-mark"))).toEqual([
    "calc(var(--s5) * var(--k) * 110 / 36)",
    "calc(var(--s5) * var(--k))",
  ]);
  expect(rule(".pen-mark")).not.toContain("--lh-");
  // after the words, in their flow, moved down only relatively, so the line keeps its height
  const tail = rule(".working-pen");
  expect(tail).toContain("display: inline-block;");
  expect(tail).toContain("position: relative;");
  expect(tail).not.toMatch(/\bheight|width/);
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

test("by night the pen's faded tail starts part way in, so it reads as it does by day", () => {
  const css = readFileSync(join(dirname(fileURLToPath(import.meta.url)), "live.css"), "utf8");
  const at = css.indexOf(':root[data-theme="night"] .pen-mark {');
  expect(at).toBeGreaterThan(-1);
  const night = css.slice(at, css.indexOf("}", at));
  // the day's fade starts from nothing; the night's from part of the ink
  expect(night).toContain(
    "mask-image: linear-gradient(to right, transparent -20%, var(--primary) 24%);",
  );
});
