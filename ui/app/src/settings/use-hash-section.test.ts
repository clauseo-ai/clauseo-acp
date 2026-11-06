// @vitest-environment happy-dom
// The settings window follows its hash (U15, use-hash-section.ts): a wanted section that isn't shown yet waits for the
// first connectors state, a later hash opens its section, each open a hash caused focuses the section's head, and a
// press on the side writes the hash without moving focus.
import { act } from "react";
import { afterEach, expect, test, vi } from "vitest";
import { stubFonts } from "../room/thread/testing.ts";
import { mountSettings, SIGNED_IN, stateOf, type Rig } from "./sections/connectors-rig.tsx";
import { hashId } from "./use-hash-section.ts";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
stubFonts();
vi.mock("../transport/window-title.ts", () => ({ setWindowTitle: () => {} }));

let rig: Rig | null = null;

afterEach(async () => {
  await rig?.unmount();
  rig = null;
});

const head = (r: Rig) => r.el.querySelector("main h2");
const firstName = (r: Rig) => r.el.querySelector(".set-nav-name")?.textContent;

/** The host's move of an open window: `location.hash = "connectors"`, and the event it fires. */
async function hostHash(id: string) {
  await act(async () => {
    history.replaceState(null, "", `/settings.html#${id}`);
    globalThis.dispatchEvent(new HashChangeEvent("hashchange"));
  });
}

test("a hash is a section's id", () => {
  expect(hashId("#connectors")).toBe("connectors");
  expect(hashId("")).toBe("");
});

test("opened at #connectors before any state: the first section, then the first state listing it opens Connectors and focuses its head", async () => {
  const r = await mountSettings({ hash: "#connectors" });
  rig = r;
  expect(head(r)?.textContent).toBe(firstName(r));
  expect(head(r)?.textContent).not.toBe("Connectors");
  await r.play(stateOf({ status: SIGNED_IN }));
  expect(head(r)?.textContent).toBe("Connectors");
  expect(document.activeElement).toBe(head(r));
});

test("open on Appearance, a hashchange to #connectors opens Connectors and focuses its head", async () => {
  const r = await mountSettings({ state: stateOf({}), hash: "#appearance" });
  rig = r;
  expect(head(r)?.textContent).toBe("Appearance");
  (document.activeElement as HTMLElement | null)?.blur();
  await hostHash("connectors");
  expect(head(r)?.textContent).toBe("Connectors");
  expect(document.activeElement).toBe(head(r));
});

test("#connectors with a state listing none opens the first section, and a later state listing it doesn't jump", async () => {
  const r = await mountSettings({
    state: { available: true, locked: false, connectors: [] },
    hash: "#connectors",
  });
  rig = r;
  expect(head(r)?.textContent).toBe(firstName(r));
  await r.play(stateOf({}));
  expect([...r.el.querySelectorAll(".set-nav-name")].map((n) => n.textContent)).toContain(
    "Connectors",
  );
  expect(head(r)?.textContent).not.toBe("Connectors");
  expect(document.activeElement).not.toBe(head(r));
});

test("a press on the side writes the hash and moves no focus to the head", async () => {
  const r = await mountSettings({ state: stateOf({}) });
  rig = r;
  await r.side("Connectors");
  expect(location.hash).toBe("#connectors");
  expect(head(r)?.textContent).toBe("Connectors");
  expect(document.activeElement).not.toBe(head(r));
  await r.side("Appearance");
  expect(location.hash).toBe("#appearance");
  expect(document.activeElement).not.toBe(head(r));
});
