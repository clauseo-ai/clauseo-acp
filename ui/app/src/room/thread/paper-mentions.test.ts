// The composer's `@` papers from the app's side: list_papers' papers as mentions (FileRefId, file name, folder, path),
// read only when `@` first asks and kept per transport and matter for 30 s, a failed read not kept; and a send's
// prompt, the text then one File block per paper, each once, in order, a chip's paper that is also a token once.
import { expect, test } from "vitest";
import { uuid } from "../../testing/stream-script.ts";
import { FakeTransport } from "../../transport/fake-transport.ts";
import type { PaperSummary } from "../../transport/protocol.ts";
import type { NextQuery, Papers } from "../host-next.ts";
import {
  paperMentions,
  papersToMentions,
  promptContent,
  PAPERS_FRESH_MS,
} from "./paper-mentions.ts";

const MATTER = uuid(1, 2);
const OTHER = uuid(2, 2);
const paper = (n: number, path: string): PaperSummary => ({
  file: uuid(n, 8),
  path,
  size: 1000,
  modified: 1_730_000_000_000,
});
const PAPERS: Papers = {
  papers: [paper(1, "Notice.pdf"), paper(2, "Pleadings/Reply/Board minutes.pdf")],
  skipped: [],
  unread: [],
  truncated: false,
};

/** A fake host with H8 alone, answering list_papers from `answer`, and the list_papers it was asked. */
function host(answer: (q: NextQuery) => Papers | Promise<Papers> = () => PAPERS) {
  const transport = new FakeTransport({
    next: {
      parts: ["papers"],
      query: async (q) => ({ papers: await answer(q) }),
    },
  });
  const asked = () => transport.calls.filter((c) => c.call === "next_query").map((c) => c.query);
  return { transport, asked };
}

test("a paper is a mention: its FileRefId, its file name, its folder and its path; a short list says so", () => {
  expect(papersToMentions(PAPERS)).toEqual({
    items: [
      { id: uuid(1, 8), label: "Notice.pdf", detail: undefined, text: "Notice.pdf" },
      {
        id: uuid(2, 8),
        label: "Board minutes.pdf",
        detail: "Pleadings/Reply",
        text: "Pleadings/Reply/Board minutes.pdf",
      },
    ],
    partial: false,
  });
  expect(papersToMentions({ ...PAPERS, truncated: true }).partial).toBe(true);
});

test("no matter, or a host without list_papers: no mentions, so @ stays a character", () => {
  expect(paperMentions(host().transport, null)).toBeUndefined();
  expect(paperMentions(new FakeTransport({}), MATTER)).toBeUndefined();
});

test("nothing is read until the first load; then the list is kept per matter for 30 s, and read again after", async () => {
  const { transport, asked } = host();
  let now = 1_000;
  const clock = () => now;
  const mentions = paperMentions(transport, MATTER, clock)!;
  expect(asked()).toEqual([]);
  const first = await mentions.load();
  expect(first.items.map((m) => m.label)).toEqual(["Notice.pdf", "Board minutes.pdf"]);
  expect(asked()).toEqual([{ list_papers: { matter: MATTER } }]);
  // another conversation's composer in the same matter shares the kept list
  now += PAPERS_FRESH_MS - 1;
  expect(await paperMentions(transport, MATTER, clock)!.load()).toBe(first);
  expect(asked()).toHaveLength(1);
  // another matter is its own
  await paperMentions(transport, OTHER, clock)!.load();
  expect(asked()).toHaveLength(2);
  now += 1;
  await mentions.load();
  expect(asked()).toEqual([
    { list_papers: { matter: MATTER } },
    { list_papers: { matter: OTHER } },
    { list_papers: { matter: MATTER } },
  ]);
});

test("a failed read is not kept: the next load asks again", async () => {
  let fail = true;
  const { transport, asked } = host(() => {
    if (fail) throw { code: "matter.folder_missing", message: "gone", retryable: false };
    return PAPERS;
  });
  const mentions = paperMentions(transport, MATTER, () => 0)!;
  await expect(mentions.load()).rejects.toBeDefined();
  fail = false;
  expect((await mentions.load()).items).toHaveLength(2);
  expect(asked()).toHaveLength(2);
});

test("a prompt is the text, then one File block per paper, in first-appearance order, each once", () => {
  const [a, b, c] = [uuid(1, 8), uuid(2, 8), uuid(3, 8)];
  expect(promptContent("q")).toEqual([{ text: { text: "q" } }]);
  expect(promptContent("q", [b, a, b])).toEqual([
    { text: { text: "q" } },
    { file: { file: b } },
    { file: { file: a } },
  ]);
  // a chip's paper (after the tokens' papers) that is also a token goes once, where the token put it
  const tokens = [a, b];
  const chips = [c, a];
  expect(promptContent("q", [...tokens, ...chips])).toEqual([
    { text: { text: "q" } },
    { file: { file: a } },
    { file: { file: b } },
    { file: { file: c } },
  ]);
});
