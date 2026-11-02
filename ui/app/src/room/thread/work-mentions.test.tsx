// @vitest-environment happy-dom
// The composer's `@` papers in the room over FakeTransport: the papers come from list_papers, asked for by the
// composer only once `@` opens its menu; a picked paper is a token, and Enter sends its words with one File block per
// paper its tokens still mention, each once, in order, however it goes: asked, queued while Clauseo works (Enter or the
// hint's Queue), or steered (Send now). A deleted token sends no File block. The open menu passes axe.
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import axe from "axe-core";
import { act } from "react";
import { settled } from "@clauseo/components/lib/late.ts";
import { afterEach, beforeAll, expect, test } from "vitest";
import { header, reset, THREAD, userMessage, uuid } from "../../testing/stream-script.ts";
import type {
  CommandOutcome,
  HostCommand,
  PaperSummary,
  ViewEntry,
} from "../../transport/protocol.ts";
import { defaultOutcome, mountRoom, THE_MATTER, type Harness } from "./harness.ts";

let h: Harness | null = null;
beforeAll(() => {
  // the page's own title, as index.html gives it, for axe's document-title rule
  const html = readFileSync(
    join(dirname(fileURLToPath(import.meta.url)), "../../../index.html"),
    "utf8",
  );
  document.title = /<title>([^<]*)<\/title>/.exec(html)?.[1] ?? "";
});
afterEach(async () => {
  await h?.unmount();
  h = null;
});

const paper = (n: number, path: string): PaperSummary => ({
  file: uuid(n, 8),
  path,
  size: 1000,
  modified: 1_730_000_000_000,
});
const PAPERS = [paper(1, "Notice.pdf"), paper(2, "Pleadings/Board minutes.pdf")];

const field = () => h!.one("composer") as HTMLTextAreaElement;
// the menu is portalled to the page's layer, outside the room's host
const rows = () => [
  ...document.querySelectorAll<HTMLElement>('[data-testid="composer-menu-item"]'),
];
/** The list_papers the room asked, in order. */
const listPapers = () =>
  h!.transport.calls.flatMap((c) =>
    c.call === "next_query" && typeof c.query === "object" && "list_papers" in c.query
      ? [c.query]
      : [],
  );

/** Words typed with the caret at their end, as an engine's insertion. */
const type = (value: string) =>
  act(async () => {
    const a = field();
    Reflect.set(HTMLTextAreaElement.prototype, "value", value, a);
    a.setSelectionRange(value.length, value.length);
    a.dispatchEvent(new InputEvent("input", { bubbles: true, inputType: "insertText" }));
  });

/** The thread's header while Clauseo works, taking a steer into its turn. */
const RUNNING: ViewEntry = {
  header: { ...header().header!, status: "running", steer_joins_turn: true } as NonNullable<
    ViewEntry["header"]
  >,
};
/** send_prompt accepted for a turn of its own, as the host takes one queued or steered; the rest as by default. */
const acceptBehind = (c: HostCommand): CommandOutcome =>
  c.send_prompt !== undefined
    ? { prompt_accepted: { turn: uuid(2, 4), message: uuid(9, 5) } }
    : defaultOutcome(c);

async function room(busy = false) {
  h = await mountRoom({
    ...(busy ? { dispatch: acceptBehind } : {}),
    next: {
      parts: ["papers"],
      query: (q) => {
        if (typeof q === "object" && "list_papers" in q)
          return { papers: { papers: PAPERS, skipped: [], unread: [], truncated: false } };
        throw new Error(`not scripted: ${JSON.stringify(q)}`);
      },
    },
  });
  h.deliver(busy ? [reset([userMessage(1, 1, "Q")], RUNNING)] : [reset([])]);
  await act(async () => field().focus());
  await act(settled);
  await h.settle();
}

test("@ lists the matter's papers from list_papers, asked for as it opens; Enter sends the words and File blocks", async () => {
  await room();
  // the wall's own read, at mount; the composer's waits for its @
  const atMount = listPapers().length;
  await type("Compare @");
  await h!.settle();
  expect(listPapers()).toHaveLength(atMount + 1);
  expect(listPapers().at(-1)).toEqual({ list_papers: { matter: THE_MATTER.matter } });
  expect(rows().map((r) => r.querySelector("bdi")?.textContent)).toEqual([
    "Notice.pdf",
    "Board minutes.pdf",
  ]);
  await type("Compare @bo");
  expect(await h!.key({ key: "Enter", keyCode: 13 })).toBe(true);
  expect(field().value).toBe('Compare @"Pleadings/Board minutes.pdf" ');
  expect(h!.sent("send_prompt")).toEqual([]);
  await type('Compare @"Pleadings/Board minutes.pdf" with @no');
  await h!.key({ key: "Enter", keyCode: 13 });
  await type(`${field().value}and @bo`);
  await h!.key({ key: "Enter", keyCode: 13 });
  // a second @ within 30 s took the kept list
  expect(listPapers()).toHaveLength(atMount + 1);
  const words = field().value;
  expect(await h!.key({ key: "Enter", keyCode: 13 })).toBe(true);
  expect(h!.sent("send_prompt")).toEqual([
    {
      thread: THREAD,
      content: [
        { text: { text: words } },
        { file: { file: uuid(2, 8) } },
        { file: { file: uuid(1, 8) } },
      ],
    },
  ]);
});

/** Two papers picked into the field: Board minutes, then Notice, then words after them. */
async function pickTwo(): Promise<string> {
  await type("Compare @bo");
  await h!.settle();
  await h!.key({ key: "Enter", keyCode: 13 });
  await type(`${field().value}with @no`);
  await h!.key({ key: "Enter", keyCode: 13 });
  await type(`${field().value}again`);
  return field().value;
}
const BOTH = [{ file: { file: uuid(2, 8) } }, { file: { file: uuid(1, 8) } }];

test("while Clauseo works, Enter queues the words with their File blocks (send_prompt with no delivery)", async () => {
  await room(true);
  const words = await pickTwo();
  expect(h!.sent("send_prompt")).toEqual([]);
  expect(await h!.key({ key: "Enter", keyCode: 13 })).toBe(true);
  expect(h!.sent("send_prompt")).toEqual([
    { thread: THREAD, content: [{ text: { text: words } }, ...BOTH] },
  ]);
  expect(field().value).toBe("");
});

test("the hint's Queue queues the words with their File blocks", async () => {
  await room(true);
  const words = await pickTwo();
  await h!.click(h!.one("queue"));
  expect(h!.sent("send_prompt")).toEqual([
    { thread: THREAD, content: [{ text: { text: words } }, ...BOTH] },
  ]);
});

test("the hint's Send now steers the words with their File blocks", async () => {
  await room(true);
  const words = await pickTwo();
  await h!.click(h!.one("steer-send-now"));
  expect(h!.sent("send_prompt")).toEqual([
    { thread: THREAD, content: [{ text: { text: words } }, ...BOTH], delivery: "steer" },
  ]);
});

test("a token deleted before the send sends no File block", async () => {
  await room();
  await type("Read @no");
  await h!.settle();
  await h!.key({ key: "Enter", keyCode: 13 });
  expect(field().value).toBe("Read @Notice.pdf ");
  // a character typed into the token makes it plain words
  await type("Read @Notices.pdf ");
  expect(await h!.key({ key: "Enter", keyCode: 13 })).toBe(true);
  expect(h!.sent("send_prompt")).toEqual([
    { thread: THREAD, content: [{ text: { text: "Read @Notices.pdf " } }] },
  ]);
});

for (const [edge, typed] of [
  ["end", "Read @Notice.pdfx"],
  ["start", "Read y@Notice.pdf "],
] as const) {
  test(`a token typed against at its ${edge} sends no File block`, async () => {
    await room();
    await type("Read @no");
    await h!.settle();
    await h!.key({ key: "Enter", keyCode: 13 });
    expect(field().value).toBe("Read @Notice.pdf ");
    await type(typed);
    expect(await h!.key({ key: "Enter", keyCode: 13 })).toBe(true);
    expect(h!.sent("send_prompt")).toEqual([
      { thread: THREAD, content: [{ text: { text: typed } }] },
    ]);
  });
}

test("the open @ menu, with its rows, passes axe; the field names it and its lit row", async () => {
  await room();
  await type("@");
  await h!.settle();
  expect(rows()).toHaveLength(2);
  const list = document.querySelector('[role="listbox"]');
  expect(field().getAttribute("aria-controls")).toBe(list?.id);
  expect(field().getAttribute("aria-activedescendant")).toBe(rows()[0]?.id);
  const results = await axe.run(document, { resultTypes: ["violations"] });
  expect(
    results.violations.map(
      (v) => `${v.id} at ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`,
    ),
  ).toEqual([]);
});
