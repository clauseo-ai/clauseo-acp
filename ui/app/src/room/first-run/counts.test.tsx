// UD1, Done-when 1: the counts switch on by default; no command before the first act; one `first_act` with the switch's
// value at it and none after; a locked `privacy.counts` shown off with no control; the switch after the first act
// writing `privacy.counts`; and the `count_ui` events, once each, with no `first_act` before the first act.
// @vitest-environment happy-dom
import { expect, test } from "vitest";
import {
  CLAUDE,
  READY,
  SIGNED_IN,
  button,
  byId,
  counts,
  install,
  mountFirstRun,
  play,
  press,
  sent,
  settingsView,
  signIn,
  text,
  typeInto,
} from "./rig.tsx";

const toggle = () => byId("counts-switch")?.querySelector("button") ?? null;
const onOf = () => byId("counts-switch")?.dataset["state"];

test("welcome: the one question, the lede, and the counts switch on by default; no Safe workspace line", async () => {
  await mountFirstRun({ matters: [] });
  expect(text()).toContain(
    "Clauseo works on your matters with the Claude or ChatGPT plan you already pay for.",
  );
  expect(button("Open the folder for a matter")).not.toBeNull();
  expect(button("Start a new matter")).not.toBeNull();
  expect(onOf()).toBe("on");
  expect(toggle()?.getAttribute("aria-checked")).toBe("true");
  expect(text()).not.toMatch(/Safe workspace/i);
  expect(text()).not.toMatch(/agent/i);
});

test("counts: before the first act the switch is the window's alone: no command, no settings write", async () => {
  const t = await mountFirstRun({ matters: [] });
  await press(toggle());
  expect(onOf()).toBe("off");
  await press(toggle());
  await press(toggle());
  expect(onOf()).toBe("off");
  expect(sent(t, "first_act")).toEqual([]);
  expect(sent(t, "set_privacy_settings")).toEqual([]);
});

test("counts: the first act (a folder chosen) sends one first_act with the switch's value, and none after", async () => {
  const t = await mountFirstRun({ matters: [] });
  await press(toggle());
  await press("Open the folder for a matter");
  expect(sent(t, "first_act")).toEqual([{ counts: false }]);
  expect(sent(t, "create_matter")).toEqual([
    { name: "Rathi Steels", source: { existing_folder: { folder: "folder-ref-1" } } },
  ]);
  // the room opens on the matter the moment it is made
  expect(byId("matter")?.textContent).toBe("Rathi Steels");
  expect(byId("counts-switch")).toBeNull();
  expect(sent(t, "first_act")).toHaveLength(1);
});

test("counts: a new matter is the first act too, with the switch left on", async () => {
  const t = await mountFirstRun({
    matters: [],
    picked: { folder: "parent-ref", display: "Documents" },
  });
  await press("Start a new matter");
  await typeInto(byId<HTMLInputElement>("matter-name") as HTMLInputElement, "Mehta v. Mehta");
  await press(byId("create-matter"));
  expect(sent(t, "first_act")).toEqual([{ counts: true }]);
  expect(sent(t, "create_matter")).toEqual([
    { name: "Mehta v. Mehta", source: { new_folder: { folder: "parent-ref" } } },
  ]);
  expect(counts(t).filter((c) => c.onboarding_step !== undefined)).toEqual([
    { onboarding_step: { step: "welcome_shown" } },
    { onboarding_step: { step: "new_matter" } },
  ]);
});

test("counts: a cancelled folder picker is no first act", async () => {
  const t = await mountFirstRun({ matters: [], picked: null });
  await press("Open the folder for a matter");
  expect(sent(t, "first_act")).toEqual([]);
  expect(counts(t).map((c) => c.onboarding_step?.step)).toEqual(["welcome_shown"]);
});

test("counts: a locked privacy.counts shows off, with no control", async () => {
  const t = await mountFirstRun({
    matters: [],
    settings: settingsView({ locked: ["privacy.counts"] }),
  });
  expect(byId("counts-switch")).toBeNull();
  expect(document.querySelector('[role="switch"]')).toBeNull();
  expect(text()).toContain("Your firm keeps anonymous counts off.");
  await press("Open the folder for a matter");
  expect(sent(t, "first_act")).toEqual([{ counts: false }]);
});

test("counts: after the first act (counts_started), the switch reads privacy.counts and writes SetPrivacySettings, and sends no first_act", async () => {
  const t = await mountFirstRun({
    matters: [],
    settings: settingsView({ counts: false, started: true }),
  });
  expect(onOf()).toBe("off");
  await press(toggle());
  expect(sent(t, "set_privacy_settings")).toEqual([{ patch: { counts: true } }]);
  await press("Open the folder for a matter");
  expect(sent(t, "first_act")).toEqual([]);
});

test("count_ui: welcome_shown, folder_chosen, agent_ready and signed_in, once each, and no first_act before the first act", async () => {
  const t = await mountFirstRun({
    matters: [],
    first: [install(CLAUDE, "installing"), signIn(CLAUDE, { signed_out: {} })],
  });
  expect(counts(t)).toEqual([{ onboarding_step: { step: "welcome_shown" } }]);
  await play(t, install(CLAUDE, READY));
  // counts before the first act: the host holds them in memory (S21); still no first_act
  expect(sent(t, "first_act")).toEqual([]);
  await press("Open the folder for a matter");
  await play(t, signIn(CLAUDE, SIGNED_IN), install(CLAUDE, READY), signIn(CLAUDE, SIGNED_IN));
  expect(counts(t)).toEqual([
    { onboarding_step: { step: "welcome_shown" } },
    { onboarding_step: { step: "agent_ready" } },
    { onboarding_step: { step: "folder_chosen" } },
    { onboarding_step: { step: "signed_in" } },
  ]);
  expect(sent(t, "first_act")).toHaveLength(1);
});
