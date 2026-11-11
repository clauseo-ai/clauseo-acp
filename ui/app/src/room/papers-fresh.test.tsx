// @vitest-environment happy-dom
// The wall's papers stay current: a file the agent made, or one a restore or undo brought back or took away, shows
// once the matter's stream gains its version, with no new conversation; the stream's first window lists nothing more.
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { MATTER, upsert } from "../testing/stream-script.ts";
import type { PaperSummary } from "./host-next.ts";
import { detailed, fakeNext } from "./fake-next.ts";
import {
  file,
  matterReset,
  MATTER_STREAM,
  mountVersions,
  NOON,
  THE_MATTER,
  version,
  type VersionsHarness,
} from "./versions/testing.ts";

let h: VersionsHarness | null = null;
beforeEach(() => {
  vi.setSystemTime(NOON + 30 * 60_000);
});
afterEach(async () => {
  await h?.unmount();
  h = null;
  vi.useRealTimers();
});

const paper = (n: number, path: string): PaperSummary => ({
  file: `ref-${n}`,
  path,
  size: 100,
  modified: NOON,
});

const tiles = () =>
  [...document.body.querySelectorAll("li.tile .tile-title")].map((t) => t.textContent);

test("a file the agent made shows as a tile once the matter's stream has its version", async () => {
  const papers: PaperSummary[] = [paper(1, "Notice.docx")];
  h = await mountVersions([], [], {
    next: fakeNext({ matters: () => [detailed(THE_MATTER)], papers, parts: ["papers"] }),
  });
  const listed = () =>
    h?.transport
      .only("next_query")
      .filter((q) => typeof q.query === "object" && "list_papers" in q.query);
  expect(tiles()).toEqual(["Notice.docx"]);
  // the matter's stream, held for the papers, arrives: nothing changed, so nothing is listed again
  await h.deliver(MATTER_STREAM, [matterReset([version(1, "import")])]);
  expect(listed()).toHaveLength(1);
  // the agent's turn writes a file, and its version lands
  papers.push(paper(2, "Reply notes.md"));
  await h.deliver(MATTER_STREAM, [
    upsert(version(2, "after_turn", { files: [file("Reply notes.md", { change: "added" })] })),
  ]);
  expect(listed()).toEqual([
    { call: "next_query", query: { list_papers: { matter: MATTER } } },
    { call: "next_query", query: { list_papers: { matter: MATTER } } },
  ]);
  expect(tiles()).toEqual(["Notice.docx", "Reply notes.md"]);
});
