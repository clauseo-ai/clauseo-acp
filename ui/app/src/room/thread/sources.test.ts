// The web sources' fold: each tool call's web (ToolCallWeb, the host's planned shape) in seq order, an envelope seen
// twice counted once, tool calls and threads apart, a way from a newer Clauseo read as unknown, and every other
// event passed over.
import { expect, test } from "vitest";
import { STREAM, uuid } from "../../testing/stream-script.ts";
import { searched, visited } from "../../testing/web-sources.ts";
import type { Envelope } from "../../transport/protocol.ts";
import { foldWebSources } from "./sources.ts";

const OTHER_STREAM = `thread:${uuid(2, 1)}`;
const webOf = (envelopes: Envelope[], call: string, stream = STREAM) =>
  foldWebSources(envelopes).get(stream)?.get(call);

test("a tool call's search and pages come in seq order, whatever order they arrived in", () => {
  const envelopes = [
    visited(3, "https://example.com/b", { call: "c" }),
    searched(1, "notice period", [{ title: "A", url: "https://example.com/a" }], { call: "c" }),
    visited(2, "https://example.com/a", { title: "A", call: "c" }),
  ];
  expect(webOf(envelopes, "c")).toEqual({
    query: "notice period",
    results: [{ title: "A", url: "https://example.com/a" }],
    pages: [
      { url: "https://example.com/a", title: "A", via: "web_fetch" },
      { url: "https://example.com/b", via: "web_fetch" },
    ],
  });
});

test("an envelope delivered twice counts once", () => {
  const twice = visited(2, "https://example.com", { call: "c" });
  expect(webOf([twice, twice], "c")?.pages.length).toBe(1);
});

test("each tool call and each thread has its own", () => {
  const folded = foldWebSources([
    searched(1, "first"),
    visited(2, "https://example.com"),
    searched(1, "elsewhere", [], { stream: OTHER_STREAM }),
  ]);
  expect(folded.get(STREAM)?.get("call-1")).toEqual({ query: "first", results: [], pages: [] });
  expect(folded.get(STREAM)?.get("call-2")?.query).toBeUndefined();
  // the same seq on another stream is another envelope
  expect(folded.get(OTHER_STREAM)?.get("call-1")?.query).toBe("elsewhere");
});

test("a way of opening a page this version doesn't know reads as unknown", () => {
  const page = visited(1, "https://example.com", { via: "teleport" as never });
  expect(webOf([page], "call-1")?.pages).toEqual([{ url: "https://example.com", via: "unknown" }]);
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
  expect([...(folded.get(STREAM)?.keys() ?? [])]).toEqual(["call-3"]);
  expect(foldWebSources([other(1, { thread_renamed: { title: "Lease" } })]).size).toBe(0);
});
