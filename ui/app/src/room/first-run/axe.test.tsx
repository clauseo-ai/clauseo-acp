// UD1: axe-core over the first run's first screen and its sign-in gate, as room/axe.test.tsx runs it over the room:
// every violation, page-has-heading-one included (the first screen's header has no title, so its h1 is the question's,
// visually hidden). happy-dom computes no styles, so colour contrast is the review page's to check.
// @vitest-environment happy-dom
import axe from "axe-core";
import { expect, test } from "vitest";
import { CLAUDE, READY, byId, install, mountFirstRun, press, signIn } from "./rig.tsx";

async function violations(): Promise<string[]> {
  document.title = "Clauseo";
  const results = await axe.run(document, { resultTypes: ["violations"] });
  return results.violations.map(
    (v) => `${v.id} at ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`,
  );
}

test("axe: the first screen has one level-one heading and no violation, downloading and with the new matter form", async () => {
  await mountFirstRun({
    matters: [],
    first: [
      install(CLAUDE, { downloading: { received: 1, total: 200e6 } }),
      signIn(CLAUDE, { signed_out: {} }),
    ],
  });
  expect([...document.querySelectorAll("h1")].map((h) => h.textContent)).toEqual([
    "Open the first matter",
  ]);
  expect(await violations()).toEqual([]);
  await press(byId("start-new-matter"));
  expect(byId("matter-name")).not.toBeNull();
  expect(document.querySelectorAll("h1")).toHaveLength(1);
  expect(await violations()).toEqual([]);
});

test("axe: the sign-in gate and the key panel in the composer have no violation", async () => {
  await mountFirstRun({ first: [install(CLAUDE, READY), signIn(CLAUDE, { signed_out: {} })] });
  expect(byId("signin")).not.toBeNull();
  expect(await violations()).toEqual([]);
  await press("Use an API key or my firm's account");
  expect(byId("signin-key")).not.toBeNull();
  expect(await violations()).toEqual([]);
});
