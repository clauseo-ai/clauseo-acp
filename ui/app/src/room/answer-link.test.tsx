// @vitest-environment happy-dom
// A relative link in an answer (the agent's own markdown, `[Reply notes.md](Reply%20notes.md)`) opens the paper at that
// path; one the matter's folder no longer holds, even in a fresh list, says so on the composer's line, never nothing.
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { afterEach, expect, test, vi } from "vitest";
import { agentText, reset, userMessage, uuid } from "../testing/stream-script.ts";
import type { PaperSummary } from "../transport/protocol.ts";
import { detailed, fakeNext } from "./fake-next.ts";
import { PAPER_GONE } from "./open.ts";
import { mountRoom, summary, THE_MATTER, type Harness } from "./thread/harness.ts";

let h: Harness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
  vi.restoreAllMocks();
});

const NOTICE = uuid(17, 8);
const NOTES = uuid(18, 8);

const linkNamed = (text: string) =>
  [...(h?.host.querySelectorAll<HTMLElement>("a[data-clauseo='link']") ?? [])].find(
    (a) => a.textContent === text,
  );
const listed = () =>
  h?.transport
    .only("next_query")
    .filter((q) => typeof q.query === "object" && "list_papers" in q.query) ?? [];

test("a relative link opens its paper; one no longer in the folder says so on the composer's line", async () => {
  vi.spyOn(console, "warn").mockImplementation(() => {});
  const papers: PaperSummary[] = [
    { file: NOTICE, path: "drafts/Notice.docx", size: 1, modified: 0 },
  ];
  h = await mountRoom({
    next: fakeNext({
      matters: () => [detailed(THE_MATTER)],
      threads: () => [{ ...summary(), seen: true }],
      papers,
    }),
  });
  h.deliver([
    reset([
      userMessage(1, 1, "Make reply notes"),
      agentText(
        2,
        1,
        "Made [Reply notes.md](Reply%20notes.md) beside [the notice](./drafts/Notice.docx).",
      ),
    ]),
  ]);
  await h.settle();
  // a paper the listed papers hold opens by its file ref
  await h.click(linkNamed("the notice"));
  expect(h.transport.only("open_file")).toEqual([{ call: "open_file", file: NOTICE }]);
  // a paper they don't: a fresh list decides, and it isn't there either
  const before = listed().length;
  await h.click(linkNamed("Reply notes.md"));
  await h.settle();
  expect(listed()).toHaveLength(before + 1);
  expect(h.transport.only("open_file")).toHaveLength(1);
  expect(h.one("error")?.textContent).toContain(errorWords("en", PAPER_GONE));
  expect(errorWords("en", PAPER_GONE)).toBe("This paper is no longer in the matter's folder.");
  // the folder holds it again: the fresh list finds it, it opens, and the line goes
  papers.push({ file: NOTES, path: "Reply notes.md", size: 1, modified: 0 });
  await h.click(linkNamed("Reply notes.md"));
  await h.settle();
  expect(h.transport.only("open_file").at(-1)).toEqual({ call: "open_file", file: NOTES });
  expect(h.one("error")).toBeNull();
});
