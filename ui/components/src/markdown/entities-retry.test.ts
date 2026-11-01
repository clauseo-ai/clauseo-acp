import { afterEach, expect, test, vi } from "vitest";

// A failed fetch of the full entity table is tried again: the renderer's prefetch (entities.ts) schedules
// anew, and the patched decoder (ui/patches/) imports anew instead of keeping the rejected promise.

const loader = vi.hoisted(() => ({ calls: 0, fail: true }));
vi.mock("decode-named-character-reference", () => ({
  characterEntitiesLoaded: () => false,
  characterReferenceMisses: () => 0,
  onCharacterEntities: () => () => {},
  loadCharacterEntities: () => {
    loader.calls++;
    return loader.fail ? Promise.reject(new Error("chunk blocked")) : Promise.resolve();
  },
}));

afterEach(() => vi.useRealTimers());

test("the prefetch schedules once, and again after a failed load", async () => {
  vi.useFakeTimers();
  const { prefetchEntities } = await import("./entities.ts");
  prefetchEntities();
  prefetchEntities();
  await vi.runAllTimersAsync();
  expect(loader.calls).toBe(1);
  loader.fail = false;
  prefetchEntities();
  await vi.runAllTimersAsync();
  expect(loader.calls).toBe(2);
  prefetchEntities();
  await vi.runAllTimersAsync();
  expect(loader.calls).toBe(2);
});
