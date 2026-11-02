// @vitest-environment happy-dom
// One slice of the hostile cases, the one this file's name gives (hostile-dom-fixture.tsx's streamThisSlice), each
// streamed in its deltas with G4's DOM checks after every delta, before the entity table has loaded
// (hostile-all.test.tsx streams the cases the table changes once it has).
import { vi } from "vitest";
import { streamThisSlice } from "./hostile-dom-fixture.tsx";

// The component's idle prefetch would load the table.
vi.mock("./entities.ts", async (original) => ({
  ...(await original<typeof import("./entities.ts")>()),
  prefetchEntities: () => {},
}));

streamThisSlice();
