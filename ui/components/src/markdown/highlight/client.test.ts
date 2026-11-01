import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { createHighlightClient } from "./client.ts";
import { fakeFactory } from "./worker-fixture.ts";
import type { Lines } from "./languages.ts";

const TOKENS: Lines = [
  [
    [2, "const"],
    [0, " a = "],
    [4, "1"],
  ],
];

beforeEach(() => {
  vi.useFakeTimers();
});
afterEach(() => {
  vi.useRealTimers();
});

test("creating the client starts no worker; the first block in a known language starts one", async () => {
  const factory = fakeFactory();
  const client = createHighlightClient(factory);
  expect(factory.workers).toHaveLength(0);
  const tokens = client.highlight("const a = 1", "ts");
  expect(factory.workers).toHaveLength(1);
  expect(factory.workers[0]?.asked).toEqual([
    { id: 1, language: "typescript", code: "const a = 1" },
  ]);
  factory.workers[0]?.answer(TOKENS);
  await expect(tokens).resolves.toEqual(TOKENS);
});

test("a language outside the eight stays plain and starts nothing", async () => {
  const factory = fakeFactory();
  const client = createHighlightClient(factory);
  await expect(client.highlight("fn main() {}", "rust")).resolves.toBeNull();
  await expect(client.highlight("x", "")).resolves.toBeNull();
  expect(factory.workers).toHaveLength(0);
});

test("a fence named after an Object property is an unknown language, and later blocks still colour", async () => {
  const factory = fakeFactory();
  const client = createHighlightClient(factory);
  for (const name of ["constructor", "__proto__", "toString", "hasOwnProperty", "valueOf"]) {
    await expect(client.highlight("x", name)).resolves.toBeNull();
    expect(client.peek("x", name)).toBeUndefined();
  }
  expect(factory.workers).toHaveLength(0);
  const json = client.highlight("{}", "json");
  factory.workers[0]?.answer(TOKENS);
  await expect(json).resolves.toEqual(TOKENS);
});

test("one worker for every block, asked one block at a time", async () => {
  const factory = fakeFactory();
  const client = createHighlightClient(factory);
  const first = client.highlight("a", "json");
  const second = client.highlight("b", "python");
  expect(factory.workers).toHaveLength(1);
  const worker = factory.workers[0];
  expect(worker?.asked.map((r) => r.code)).toEqual(["a"]);
  worker?.answer(TOKENS);
  await expect(first).resolves.toEqual(TOKENS);
  expect(worker?.asked.map((r) => r.code)).toEqual(["b"]);
  worker?.answer(null);
  await expect(second).resolves.toBeNull();
  expect(factory.workers).toHaveLength(1);
});

test("a block seen before comes from the LRU without asking the worker", async () => {
  const factory = fakeFactory();
  const client = createHighlightClient(factory);
  expect(client.peek("const a = 1", "typescript")).toBeUndefined();
  const first = client.highlight("const a = 1", "typescript");
  factory.workers[0]?.answer(TOKENS);
  await first;
  // ts and typescript are one grammar, so one entry.
  expect(client.peek("const a = 1", "ts")).toEqual(TOKENS);
  await expect(client.highlight("const a = 1", "tsx")).resolves.toEqual(TOKENS);
  expect(factory.workers[0]?.asked).toEqual([]);
});

test("the same block asked twice while in flight is sent once", async () => {
  const factory = fakeFactory();
  const client = createHighlightClient(factory);
  const a = client.highlight("x", "sql");
  const b = client.highlight("x", "sql");
  expect(factory.workers[0]?.asked).toHaveLength(1);
  factory.workers[0]?.answer(TOKENS);
  await expect(Promise.all([a, b])).resolves.toEqual([TOKENS, TOKENS]);
});

test("the LRU keeps the last blocks used and drops the oldest", async () => {
  const factory = fakeFactory();
  const client = createHighlightClient({ ...factory, limit: 2 });
  for (const code of ["a", "b"]) {
    const done = client.highlight(code, "json");
    factory.workers[0]?.answer(TOKENS);
    await done;
  }
  expect(client.peek("a", "json")).toEqual(TOKENS); // a is now the newest
  const done = client.highlight("c", "json");
  factory.workers[0]?.answer(TOKENS);
  await done;
  expect(client.peek("b", "json")).toBeUndefined();
  expect(client.peek("a", "json")).toEqual(TOKENS);
  expect(client.peek("c", "json")).toEqual(TOKENS);
});

test("a block that misses its deadline stays plain, is not asked again, and a fresh worker takes the rest", async () => {
  const factory = fakeFactory();
  const client = createHighlightClient({ ...factory, timeoutMs: 1000 });
  const slow = client.highlight("slow", "bash");
  const next = client.highlight("next", "bash");
  await vi.advanceTimersByTimeAsync(999);
  expect(factory.workers).toHaveLength(1);
  await vi.advanceTimersByTimeAsync(1);
  await expect(slow).resolves.toBeNull();
  expect(factory.workers[0]?.terminated).toBe(true);
  expect(factory.workers).toHaveLength(2);
  expect(factory.workers[1]?.asked.map((r) => r.code)).toEqual(["next"]);
  // An answer from the stuck worker after its deadline changes nothing.
  factory.workers[0]?.answer(TOKENS);
  factory.workers[1]?.answer(TOKENS);
  await expect(next).resolves.toEqual(TOKENS);
  await expect(client.highlight("slow", "bash")).resolves.toBeNull();
  expect(factory.workers[1]?.asked).toEqual([]);
});

test("after the restarts run out every block stays plain", async () => {
  const factory = fakeFactory();
  const client = createHighlightClient({ ...factory, timeoutMs: 10, restarts: 1 });
  const blocks = ["a", "b", "c"].map((code) => client.highlight(code, "xml"));
  await vi.advanceTimersByTimeAsync(20);
  await expect(Promise.all(blocks)).resolves.toEqual([null, null, null]);
  expect(factory.workers).toHaveLength(2);
  expect(factory.workers.every((w) => w.terminated)).toBe(true);
  await expect(client.highlight("d", "xml")).resolves.toBeNull();
  expect(factory.workers).toHaveLength(2);
});

test("a worker that fails to start or crashes leaves every block plain, and is never restarted", async () => {
  let starts = 0;
  const refused = createHighlightClient({
    createWorker: () => {
      starts++;
      throw new TypeError("refused script URL");
    },
  });
  await expect(refused.highlight("a", "html")).resolves.toBeNull();
  await expect(refused.highlight("b", "html")).resolves.toBeNull();
  expect(starts).toBe(1);

  const factory = fakeFactory();
  const crashing = createHighlightClient(factory);
  const blocks = [crashing.highlight("a", "md"), crashing.highlight("b", "md")];
  factory.workers[0]?.crash();
  await expect(Promise.all(blocks)).resolves.toEqual([null, null]);
  await expect(crashing.highlight("c", "md")).resolves.toBeNull();
  expect(factory.workers).toHaveLength(1);
});
