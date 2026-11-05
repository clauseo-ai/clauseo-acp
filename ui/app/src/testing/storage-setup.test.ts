// @vitest-environment happy-dom
// A DOM test's storage works whatever the Node: storage-setup.ts gives it one where Node's global localStorage is unusable.
import { expect, test } from "vitest";

test("a DOM test's localStorage and sessionStorage store and remove values", () => {
  for (const storage of [localStorage, sessionStorage]) {
    storage.setItem("probe", "1");
    expect(storage.getItem("probe")).toBe("1");
    storage.removeItem("probe");
    expect(storage.getItem("probe")).toBeNull();
  }
});
