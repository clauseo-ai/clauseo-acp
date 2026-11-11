// @vitest-environment happy-dom
// Versions' undo asks the host what changed since the turn's version (preview_undo, as the thread's Undo does): a file
// the turn added that the lawyer edited after it is kept by default, though no newer version shows the edit; a file
// left as the turn made it goes back with no question.
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { THREAD, TURN } from "../../testing/stream-script.ts";
import type { HostQuery, ViewEntry } from "../../transport/protocol.ts";
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
