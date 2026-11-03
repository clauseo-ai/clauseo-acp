// The web sources' fold: each thread's turns, in seq order, an envelope seen twice counted once, a way from a newer
// Clauseo read as unknown, and every other event passed over.
import { expect, test } from "vitest";
import { STREAM, TURN, uuid } from "../../testing/stream-script.ts";
import { searched, visited } from "../../testing/web-sources.ts";
import type { Envelope } from "../../transport/protocol.ts";
import { foldWebSources } from "./sources.ts";

const OTHER_TURN = uuid(2, 4);
const OTHER_STREAM = `thread:${uuid(2, 1)}`;

test("each turn's sources come in seq order, whatever order they arrived in", () => {
  const folded = foldWebSources([
    visited(3, "https://example.com/b"),
    searched(1, "notice period"),
    visited(2, "https://example.com/a", { title: "A" }),
  ]);
  expect(
    folded
      .get(STREAM)
      ?.get(TURN)
      ?.map((s) => s.seq),
  ).toEqual([1, 2, 3]);
  expect(folded.get(STREAM)?.get(TURN)?.[1]).toEqual({
    kind: "page",
    seq: 2,
    url: "https://example.com/a",
    title: "A",
    via: "web_fetch",
  });
});

test("an envelope delivered twice counts once", () => {
  const twice = searched(1, "notice period");
  const folded = foldWebSources([twice, visited(2, "https://example.com"), twice]);
  expect(folded.get(STREAM)?.get(TURN)?.length).toBe(2);
});

test("sources go to their own turn and their own thread", () => {
  const folded = foldWebSources([
    searched(1, "first"),
    searched(2, "second", [], { turn: OTHER_TURN }),
    searched(1, "elsewhere", [], { stream: OTHER_STREAM }),
  ]);
  expect(
    folded
      .get(STREAM)
      ?.get(TURN)
      ?.map((s) => s.kind === "search" && s.query),
  ).toEqual(["first"]);
  expect(
    folded
      .get(STREAM)
      ?.get(OTHER_TURN)
      ?.map((s) => s.kind === "search" && s.query),
  ).toEqual(["second"]);
  // the same seq on another stream is another envelope
  expect(folded.get(OTHER_STREAM)?.get(TURN)?.length).toBe(1);
});

test("a way of opening a page this version doesn't know reads as unknown", () => {
  const page = visited(1, "https://example.com", { via: "teleport" as never });
  const [source] = foldWebSources([page]).get(STREAM)?.get(TURN) ?? [];
  expect(source).toMatchObject({ kind: "page", via: "unknown", title: null });
});

test("every other event, and a kind this version doesn't know, is passed over", () => {
  const other = (seq: number, event: object): Envelope => ({
    ...searched(seq, "x"),
    event: event as Envelope["event"],
  });
  const folded = foldWebSources([
    other(1, { thread_renamed: { title: "Lease" } }),
    other(2, { a_kind_from_later: { anything: 1 } }),
    searched(3, "kept"),
  ]);
  expect(
    folded
      .get(STREAM)
      ?.get(TURN)
      ?.map((s) => s.seq),
  ).toEqual([3]);
  expect(foldWebSources([other(1, { thread_renamed: { title: "Lease" } })]).size).toBe(0);
});
