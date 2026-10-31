import { afterEach, expect, test, vi } from "vitest";
import { displayFromQuery } from "./query.ts";

afterEach(() => {
  vi.unstubAllGlobals();
});

test("the query picks any of the three platforms", () => {
  for (const p of ["mac", "windows", "linux"] as const) {
    expect(displayFromQuery(`?platform=${p}`).platform).toBe(p);
  }
});

test("without a platform in the query, the gallery guesses the computer's from its navigator", () => {
  vi.stubGlobal("navigator", { platform: "Linux x86_64", userAgent: "" });
  expect(displayFromQuery("").platform).toBe("linux");
  vi.stubGlobal("navigator", {
    userAgentData: { platform: "Windows" },
    platform: "",
    userAgent: "",
  });
  expect(displayFromQuery("?platform=beos").platform).toBe("windows");
  vi.stubGlobal("navigator", { platform: "MacIntel", userAgent: "" });
  expect(displayFromQuery("").platform).toBe("mac");
});
