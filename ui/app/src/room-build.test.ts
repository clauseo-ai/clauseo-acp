// The build switch has one meaning in two spellings: room-build.ts's ROOM_BUILD, and App.tsx's inline test, which must
// stay inline for the default build to drop the room. This fails when they disagree.
import { readFileSync } from "node:fs";
import { expect, test } from "vitest";

const SWITCH = 'import.meta.env.VITE_CLAUSEO_WINDOW === "room"';

test("App.tsx and room-build.ts test the same variable against the same value", () => {
  const read = (file: string) => readFileSync(new URL(file, import.meta.url), "utf8");
  expect(read("room-build.ts")).toContain(`export const ROOM_BUILD: boolean = ${SWITCH};`);
  expect(read("App.tsx").replace(/\s+/g, " ")).toContain(
    `const room = ${SWITCH} ? await import("./room/entry.ts")`,
  );
  // and nowhere else in the app's own code
  expect(read("main.tsx")).not.toContain("VITE_CLAUSEO_WINDOW");
});
