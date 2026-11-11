// @vitest-environment happy-dom
// Versions' undo asks the host what changed since the turn's version (preview_undo, as the thread's Undo does): a file
// the turn added that the lawyer edited after it is kept by default, though no newer version shows the edit; a file
// left as the turn made it goes back with no question.
import { act } from "react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { THREAD, TURN } from "../../testing/stream-script.ts";
import type { HostQuery, ViewEntry } from "../../transport/protocol.ts";
import { EDITED_WAIT_MS } from "./reading.ts";
import {
  answering,
  file,
  mountVersions,
  NOON,
  stepEntry,
  version,
  type VersionsHarness,
} from "./testing.ts";

let h: VersionsHarness | null = null;
beforeEach(() => {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(NOON + 30 * 60_000);
});
afterEach(async () => {
  await h?.unmount();
  h = null;
  vi.useRealTimers();
});

const VERSIONS: ViewEntry[] = [
  version(1, "import"),
  version(2, "after_turn", { thread: THREAD, turn: TURN }),
  version(3, "outside_edits", { files: [file("other.txt")] }),
];
// the reply the agent made through Bash: added, and not flagged, since it was absent when the turn began
const STEP = stepEntry(5, {
  files: [file("reply.docx", { change: "added" }), file("brief.docx")],
  checkpoint_after: "cp2",
});

const dialog = () => document.body.querySelector<HTMLElement>('[role="dialog"]');
const choiceRow = (name: string) =>
  [...(dialog()?.querySelectorAll<HTMLElement>(".ver-edited-row") ?? [])].find((r) =>
    r.querySelector(".ver-edited-name")?.textContent?.includes(name),
  );

async function openUndo(edited: string[]): Promise<HostQuery[]> {
  const asked: HostQuery[] = [];
  h = await mountVersions(VERSIONS, [STEP], {
    query: answering({
      preview_undo: (q: HostQuery) => {
        asked.push(q);
        return { undo_preview: { edited_since: edited } };
      },
    }),
  });
  await h.openVersions();
  await h.click(h.all("version").find((r) => r.dataset["checkpoint"] === "cp2"));
  await h.click(h.one("version-undo"));
  await h.settle();
  return asked;
}

test("the agent's own file the lawyer edited after the turn is kept, saying since when", async () => {
  const asked = await openUndo(["reply.docx"]);
  expect(asked).toEqual([{ preview_undo: { thread: THREAD, turn: TURN } }]);
  const row = choiceRow("reply.docx");
  expect(row).toBeDefined();
  expect(choiceRow("brief.docx")).toBeUndefined();
  expect(dialog()?.querySelector(".ver-edited-head")?.textContent).toBe(
    "1 document changed again after it.",
  );
  expect(row?.querySelector(".ver-edited-at")?.textContent).toMatch(/^Changed since .+ today$/);
  await h?.click(h.one("dialog-confirm"));
  expect(h?.sent("undo_step")).toEqual([
    {
      thread: THREAD,
      turn: TURN,
      files: [
        { path: "reply.docx", choice: "keep" },
        { path: "brief.docx", choice: "undo" },
      ],
    },
  ]);
});

test("the agent's own file left as it made it goes back with no question", async () => {
  await openUndo([]);
  expect(dialog()?.querySelector(".ver-edited-row")).toBeNull();
  await h?.click(h.one("dialog-confirm"));
  expect(h?.sent("undo_step")).toEqual([
    {
      thread: THREAD,
      turn: TURN,
      files: [
        { path: "reply.docx", choice: "undo" },
        { path: "brief.docx", choice: "undo" },
      ],
    },
  ]);
});

test("Undo waits for the host's answer, and goes ahead once it comes", async () => {
  let answer: (edited: string[]) => void = () => undefined;
  h = await mountVersions(VERSIONS, [STEP], {
    query: answering({
      preview_undo: () =>
        new Promise((resolve) => {
          answer = (edited) => resolve({ undo_preview: { edited_since: edited } });
        }),
    }),
  });
  await h.openVersions();
  await h.click(h.all("version").find((r) => r.dataset["checkpoint"] === "cp2"));
  await h.click(h.one("version-undo"));
  expect(h.one("dialog-confirm")?.getAttribute("aria-disabled")).toBe("true");
  await h.click(h.one("dialog-confirm"));
  expect(h.sent("undo_step")).toEqual([]);
  await act(async () => answer([]));
  await h.settle();
  expect(h.one("dialog-confirm")?.getAttribute("aria-disabled")).toBeNull();
  await h.click(h.one("dialog-confirm"));
  expect(h.sent("undo_step")).toHaveLength(1);
});

test("with no answer from the host, a file that may hold the lawyer's edits is kept, with the warning", async () => {
  h = await mountVersions(
    VERSIONS,
    [
      stepEntry(5, {
        files: [file("notes.txt", { change: "added", may_include_lawyer_edits: true })],
        checkpoint_after: "cp2",
      }),
    ],
    {
      query: answering({
        preview_undo: () => {
          throw new Error("refused");
        },
      }),
    },
  );
  await h.openVersions();
  await h.click(h.all("version").find((r) => r.dataset["checkpoint"] === "cp2"));
  await h.click(h.one("version-undo"));
  await h.settle();
  const row = choiceRow("notes.txt");
  expect(row?.querySelector(".ver-edited-at")?.textContent).toBe(
    "Changed during the answer, though Clauseo didn't say it was changing it.",
  );
  expect(
    [...(row?.querySelectorAll("button") ?? [])]
      .find((b) => b.textContent?.trim() === "Keep current")
      ?.getAttribute("aria-pressed"),
  ).toBe("true");
  // the check is over: only that every file is kept holds Undo back
  expect(dialog()?.textContent).toContain(
    "Nothing goes back. Every document it changed is kept as it is now.",
  );
});

test("with no answer within the wait, the flags decide and Undo goes ahead", async () => {
  // the wait's timer is held here and fired by hand; every other timer runs as ever
  let fire: (() => void) | null = null;
  const real = globalThis.setTimeout;
  const spy = vi.spyOn(globalThis, "setTimeout").mockImplementation(((
    fn: () => void,
    ms?: number,
  ) => {
    if (ms === EDITED_WAIT_MS) {
      fire = fn;
      return 0 as unknown as ReturnType<typeof setTimeout>;
    }
    return real(fn, ms);
  }) as typeof setTimeout);
  h = await mountVersions(
    VERSIONS,
    [
      stepEntry(5, {
        files: [
          file("notes.txt", { change: "added", may_include_lawyer_edits: true }),
          file("brief.docx"),
        ],
        checkpoint_after: "cp2",
      }),
    ],
    { query: answering({ preview_undo: () => new Promise(() => undefined) }) },
  );
  await h.openVersions();
  await h.click(h.all("version").find((r) => r.dataset["checkpoint"] === "cp2"));
  await h.click(h.one("version-undo"));
  expect(h.one("dialog-confirm")?.getAttribute("aria-disabled")).toBe("true");
  expect(fire).not.toBeNull();
  await act(async () => fire?.());
  await h.settle();
  spy.mockRestore();
  expect(h.one("dialog-confirm")?.getAttribute("aria-disabled")).toBeNull();
  const row = choiceRow("notes.txt");
  expect(row?.querySelector(".ver-edited-at")?.textContent).toBe(
    "Changed during the answer, though Clauseo didn't say it was changing it.",
  );
  await h.click(h.one("dialog-confirm"));
  expect(h.sent("undo_step")).toEqual([
    {
      thread: THREAD,
      turn: TURN,
      files: [
        { path: "notes.txt", choice: "keep" },
        { path: "brief.docx", choice: "undo" },
      ],
    },
  ]);
});
