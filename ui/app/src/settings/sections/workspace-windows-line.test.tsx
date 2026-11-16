// @vitest-environment happy-dom
// Settings' Safe workspace says what it can't stop on Windows (UC-820), then the Mac advice (UC-850), always while it's on (tier a and tier b), and
// never on the Mac or while the computer isn't protected.
import { pw, WINDOWS_LINE, WINDOWS_STRICT } from "@clauseo/components/lib/words-protection.ts";
import { afterEach, expect, test } from "vitest";
import { PROT, view } from "../../room/protection/fixtures.ts";
import { block, mount, one, reset, text } from "./workspace-rig.tsx";

afterEach(reset);

const LINE = "protection-platform-line";

test.each([
  ["a", PROT.a],
  ["b", PROT.b],
] as const)(
  "protected (tier %s) on Windows: UC-820 then UC-850 under what keeps Clauseo in",
  async (_t, p) => {
    await mount({ view: () => view(p) }, "windows");
    expect(one(LINE)?.textContent).toBe(`${pw("en", WINDOWS_LINE)} ${pw("en", WINDOWS_STRICT)}`);
  },
);

test("protected on the Mac: neither UC-820 nor UC-850", async () => {
  await mount({ view: () => view(PROT.a) }, "mac");
  expect(block()?.dataset["state"]).toBe("a-mac");
  expect(one(LINE)).toBeNull();
  expect(text()).not.toContain(pw("en", WINDOWS_LINE));
  expect(text()).not.toContain(pw("en", WINDOWS_STRICT));
});

test("not protected on Windows: no UC-820 or UC-850, which would promise a fence that isn't there", async () => {
  await mount({ view: () => view(PROT.needs) }, "windows");
  expect(one(LINE)).toBeNull();
});
