// @vitest-environment happy-dom
// Slice 2 of the hostile cases (hostile-dom-fixture.tsx's streamSlice), each streamed in its deltas with G4's DOM
// checks after every delta, before the entity table has loaded (hostile-all.test.tsx streams the cases the table
// changes once it has).
import { beforeAll, describe, expect, test, vi } from "vitest";
import { characterEntitiesLoaded } from "./entities.ts";
import {
  catchPayloads,
  mountSurface,
  pwned,
  streamFailures,
  streamSlice,
} from "./hostile-dom-fixture.tsx";

// The component's idle prefetch would load the table.
vi.mock("./entities.ts", async (original) => ({
  ...(await original<typeof import("./entities.ts")>()),
  prefetchEntities: () => {},
}));

catchPayloads();
const surface = mountSurface();

describe("entity table not loaded", () => {
  beforeAll(() => {
    expect(characterEntitiesLoaded()).toBe(false);
  });

  test.each(streamSlice(2).map((c) => [c.id, c] as const))(
    "%s streamed in 16-character deltas stays clean at every step",
    (_, c) => {
      expect(streamFailures(surface(), c)).toEqual([]);
      expect(pwned).toEqual([]);
    },
    30_000,
  );
});
