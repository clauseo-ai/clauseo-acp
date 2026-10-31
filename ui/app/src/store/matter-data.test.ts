import { expect, test } from "vitest";
import type { Citation, Paper } from "@clauseo/components/clauseo/matter.ts";
import { buildMatterData } from "./matter-data.ts";
import { pickReopenMatter, pickReopenThread } from "./reopen.ts";
import type { MatterSummary, ThreadSummary } from "../transport/protocol.ts";
import { uuid } from "../testing/stream-script.ts";

test("a citation naming a paper the matter does not hold is logged and dropped", () => {
  const paper = { id: "p1" } as Paper;
  const good = { id: "c1", paperId: "p1" } as Citation;
  const bad = { id: "c2", paperId: "p9" } as Citation;
  const warnings: string[] = [];
  const data = buildMatterData({ papers: [paper], citations: { a1: [good, bad] } }, (m) =>
    warnings.push(m),
  );
  expect(data.citations).toEqual({ a1: [good] });
  expect(warnings).toEqual([
    "answer a1: citation c2 names paper p9, which the matter does not hold",
  ]);
  expect(buildMatterData({ papers: [] }, () => {})).toEqual({
    papers: [],
    cites: {},
    citations: {},
    inView: null,
  });
});

test("reopen picks the newest-created matter and its newest thread that is not archived", () => {
  const matter = (n: number) => ({ matter: uuid(n, 2), name: `M${n}` }) as MatterSummary;
  const thread = (n: number, status: ThreadSummary["status"] = "idle") =>
    ({ thread: uuid(n, 1), status }) as ThreadSummary;
  expect(pickReopenMatter([matter(2), matter(9), matter(4)])?.name).toBe("M9");
  expect(pickReopenMatter([])).toBeNull();
  expect(pickReopenThread([thread(1), thread(3, "archived"), thread(2)])?.thread).toBe(uuid(2, 1));
  expect(pickReopenThread([thread(3, "archived")])).toBeNull();
});
