// @vitest-environment happy-dom
// Compare with now for a file added or removed since the version: its sheet says Added since or Removed since (not
// Changed since), the side with no paper says so, an added file opens as it is now (from a fresh list when the room's
// list doesn't hold it yet), and a removed one offers nothing to open.
import { vt } from "@clauseo/components/lib/words-versions.ts";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { THREAD, TURN, uuid } from "../../testing/stream-script.ts";
import type { FileChange, ViewEntry } from "../../transport/protocol.ts";
import type { PaperSummary } from "../host-next.ts";
import { detailed, fakeNext } from "../fake-next.ts";
import {
  file,
  mountVersions,
  NOON,
  stepEntry,
  THE_MATTER,
  version,
  type VersionsHarness,
} from "./testing.ts";

let h: VersionsHarness | null = null;
beforeEach(() => {
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
const ADDED = file("Reply notes.md", { change: "added" });
const REMOVED = file("Old memo.txt", { change: "deleted" });
const REF = uuid(3, 8);

/** Versions at cp2, compared with now, over a host whose comparison names `changes` and whose list holds `papers`. */
async function compared(changes: FileChange[], papers: PaperSummary[]): Promise<VersionsHarness> {
  const harness = await mountVersions(VERSIONS, [stepEntry(5, { checkpoint_after: "cp2" })], {
    query: (q) =>
      typeof q === "object" && q.compare_versions !== undefined
        ? { file_changes: changes }
        : undefined,
    next: fakeNext({ matters: () => [detailed(THE_MATTER)], papers, parts: ["papers"] }),
  });
  await harness.openVersions();
  await harness.lookAt("cp2");
  await harness.click(harness.button("Compare with now"));
  return harness;
}

const state = () => document.body.querySelector(".ver-pdf-state")?.textContent;
const side = (which: "then" | "now") =>
  document.body.querySelector(`.ver-pdf-side[data-side="${which}"] .ver-pdf-facts`)?.textContent;

test("a file added since says Added since, has no paper then, and opens as it is now from a fresh list", async () => {
  const papers: PaperSummary[] = [];
  h = await compared([ADDED], papers);
  const time = /Added since (.+) today/.exec(state() ?? "")?.[1] ?? "";
  expect(state()).toBe(vt("en", "Added since {time} today", { time }));
  expect(state()).not.toMatch(/Changed since/);
  expect(side("then")).toBe("This paper wasn't in the matter then.");
  expect(side("now")).toBe("");
  // the room's list predates the file: Open now asks for a fresh one, which holds it
  papers.push({ file: REF, path: "Reply notes.md", size: 10, modified: 0 });
  await h.click(h.button("Open now"));
  await h.settle();
  expect(h.transport.only("open_file")).toEqual([{ call: "open_file", file: REF }]);
});

test("an added file the folder no longer holds says so, rather than doing nothing", async () => {
  h = await compared([ADDED], []);
  await h.click(h.button("Open now"));
  await h.settle();
  expect(h.transport.only("open_file")).toEqual([]);
  expect(document.body.textContent).toContain("This paper is no longer in the matter's folder.");
});

test("a file removed since says Removed since, has no paper now, and offers nothing to open", async () => {
  h = await compared([REMOVED], [{ file: REF, path: "other.txt", size: 1, modified: 0 }]);
  expect(state()).toMatch(/^Removed since .+ today$/);
  expect(side("now")).toBe("This paper isn't in the matter now.");
  expect(side("then")).toBe("");
  expect(h.button("Open now")).toBeNull();
});
