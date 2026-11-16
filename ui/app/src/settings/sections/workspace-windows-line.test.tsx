// @vitest-environment happy-dom
// Settings' Safe workspace says what it can't stop on Windows (UC-820), always while it's on (tier a and tier b), and
// never on the Mac or while the computer isn't protected.
import { pw, WINDOWS_LINE } from "@clauseo/components/lib/words-protection.ts";
import { afterEach, expect, test } from "vitest";
import { PROT, view } from "../../room/protection/fixtures.ts";
import { block, mount, one, reset, text } from "./workspace-rig.tsx";

afterEach(reset);

const LINE = "protection-platform-line";

test.each([
  ["a", PROT.a],
  ["b", PROT.b],
] as const)("protected (tier %s) on Windows: UC-820 under what keeps Clauseo in", async (_t, p) => {
  await mount({ view: () => view(p) }, "windows");
  expect(one(LINE)?.textContent).toBe(pw("en", WINDOWS_LINE));
  expect(text()).toContain(pw("en", WINDOWS_LINE));
});

test("protected on the Mac: no UC-820", async () => {
  await mount({ view: () => view(PROT.a) }, "mac");
  expect(block()?.dataset["state"]).toBe("a-mac");
  expect(one(LINE)).toBeNull();
  expect(text()).not.toContain(pw("en", WINDOWS_LINE));
});

test("not protected on Windows: no UC-820, which would promise a fence that isn't there", async () => {
  await mount({ view: () => view(PROT.needs) }, "windows");
  expect(one(LINE)).toBeNull();
});
