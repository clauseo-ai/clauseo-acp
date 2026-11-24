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
  // the header stands with no title: "No matters yet" is the rail's alone (design, welcome.tsx)
  expect(byId("matter")?.textContent ?? "").toBe("");
  expect(text().split("No matters yet").length - 1).toBe(1);
});

test("welcome: the counts line says a crash report goes with the counts (0060, UC-750)", async () => {
  await mountFirstRun({ matters: [] });
  expect(text()).toContain(
    "How many questions, which features, how long answers take, and a short report if Clauseo crashes. Never your papers, your questions or the answers.",
  );
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
    picked: { folder: "folder-ref", display: "Documents/Mehta" },
  });
  await press("Start a new matter");
  await press(byId("matter-existing"));
  await typeInto(byId<HTMLInputElement>("matter-name") as HTMLInputElement, "Mehta family trust");
  // choosing the folder is no act yet: Create matter is
  expect(sent(t, "first_act")).toEqual([]);
  await press(byId("create-matter"));
  expect(sent(t, "first_act")).toEqual([{ counts: true }]);
  expect(sent(t, "create_matter")).toEqual([
    { name: "Mehta family trust", source: { existing_folder: { folder: "folder-ref" } } },
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

test("counts: a locked privacy.counts shows the switch off, with no control: not operable or focusable, its state still spoken", async () => {
  const t = await mountFirstRun({
    matters: [],
    settings: settingsView({ locked: ["privacy.counts"] }),
  });
  expect(onOf()).toBe("off");
  const control = toggle() as HTMLButtonElement;
  expect(control.getAttribute("role")).toBe("switch");
  expect(control.getAttribute("aria-checked")).toBe("false");
  expect(control.disabled).toBe(true);
  // not focusable: focus stays where it was
  control.focus();
  expect(document.activeElement).not.toBe(control);
  // not operable: a press changes nothing and sends nothing
  await press(control);
  expect(onOf()).toBe("off");
  expect(sent(t, "set_privacy_settings")).toEqual([]);
  expect(text()).toContain("Send anonymous counts.");
  expect(text()).toContain("Managed by firm");
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

test("D-003: What's sent opens Settings at Privacy, where the counts are said, not at Plans", async () => {
  const t = await mountFirstRun({ matters: [], openSettings: true });
  await press("What's sent");
  expect(t.only("open_settings")).toEqual([{ call: "open_settings", section: "privacy" }]);
});

test("the first screen's New matter asks New folder or Existing folder first, as the rail's does", async () => {
  const t = await mountFirstRun({
    matters: [],
    picked: { folder: "folder-ref", display: "Documents/Mehta" },
  });
  await press("Start a new matter");
  // the question alone: no name field yet, and no offer of a new folder beside a picked one
  expect(byId("matter-name")).toBeNull();
  expect(byId("matter-made")).not.toBeNull();
  await press(byId("matter-existing"));
  expect(byId<HTMLInputElement>("matter-name")?.value).toBe("Mehta");
  expect(document.body.textContent).not.toContain("Use a new folder");
  expect(document.body.textContent).toContain(
    "The folder stays where it is. Nothing is moved or copied.",
  );
  // Back goes to the question, nothing sent
  await press("Back");
  expect(byId("matter-name")).toBeNull();
  expect(byId("matter-existing")).not.toBeNull();
  expect(sent(t, "create_matter")).toEqual([]);
});
