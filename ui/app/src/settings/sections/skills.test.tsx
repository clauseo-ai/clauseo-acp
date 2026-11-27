// @vitest-environment happy-dom
// Settings › Skills over the real settings window and FakeTransport (skills-fake.ts), one test a behaviour: the list,
// the empty list, the switch, Remove through its confirm, Add a skill (each route previews first), Cancel in the
// picker, the refusal lines, a re-list on settings_changed, and no section without the picker.
import { act } from "react";
import { afterEach, expect, test, vi } from "vitest";
import { stubFonts } from "../../room/thread/testing.ts";
import { FakeTransport } from "../../transport/fake-transport.ts";
import { pack, PACKS } from "./skills-fake.ts";
import { mount, SETTINGS, type Rig } from "./skills-rig.tsx";
import { skillsShown } from "./skills-shown.ts";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
stubFonts();
vi.mock("../../transport/window-title.ts", () => ({ setWindowTitle: () => {} }));

let rig: Rig | null = null;

afterEach(async () => {
  await rig?.unmount();
  rig = null;
});

const UNUSABLE = "Clauseo can't use this skill as it is.";

test("the list: each pack's name, its source, a line when it can't load, Turn on again when changed", async () => {
  rig = await mount([...PACKS, pack("plain", "Plain pack", { state: "off" })]);
  expect(rig.nav()).toContain("Skills");
  expect(rig.pane().textContent).toContain("Skills teach Claude and ChatGPT");
  expect(rig.names()).toEqual([
    "Notice drafting",
    "Cheque bounce replies",
    "Lease review",
    "Old briefs",
    "Plain pack",
  ]);
  expect(rig.sources()).toEqual([
    "Added on this computer",
    "github.com",
    "Added on this computer",
    "Added on this computer",
    "",
  ]);
  expect(rig.states()).toEqual(["", "", UNUSABLE, UNUSABLE, ""]);
  const switches = [...rig.pane().querySelectorAll<HTMLElement>('[role="switch"]')];
  expect(switches.map((s) => s.getAttribute("aria-checked"))).toEqual([
    "true",
    "false",
    "false",
    "false",
    "false",
  ]);
  expect(rig.pane().querySelectorAll('[data-testid="skills-turn-on-again"]')).toHaveLength(1);
  // Remove for the packs added here only
  expect(
    [...rig.pane().querySelectorAll('[data-testid="skills-remove"]')].map((b) =>
      b.getAttribute("aria-label"),
    ),
  ).toEqual(["Remove Notice drafting", "Remove Lease review", "Remove Old briefs"]);
});

test("an empty list says so", async () => {
  rig = await mount([]);
  expect(rig.q('[data-testid="skills-empty"]')?.textContent).toBe("No skills added yet.");
});

test("the switch sends set_skill_pack and the list follows", async () => {
  rig = await mount(PACKS);
  await rig.click(rig.row("Cheque bounce replies")?.querySelector<HTMLElement>('[role="switch"]'));
  expect(rig.skills.sent).toEqual([{ set_skill_pack: { id: "cheque-bounce", on: true } }]);
  expect(
    rig
      .row("Cheque bounce replies")
      ?.querySelector('[role="switch"]')
      ?.getAttribute("aria-checked"),
  ).toBe("true");
});

test("Remove goes through the confirm, sends remove_skill and the row goes", async () => {
  rig = await mount(PACKS);
  await rig.click(rig.q('[data-testid="skills-remove"]'));
  expect(document.querySelector('[data-testid="skills-remove-confirm"]')?.textContent).toContain(
    "Remove Notice drafting?",
  );
  await rig.click(document.querySelector<HTMLElement>('[data-testid="skills-remove-ok"]'));
  expect(rig.skills.sent).toEqual([{ remove_skill: { id: "notice-drafting" } }]);
  expect(rig.names()).not.toContain("Notice drafting");
});

test("Choose a folder previews the folder first, then Add sends add_skills and lists the new skills", async () => {
  rig = await mount([]);
  expect(rig.q('[data-testid="add-skill"]')?.textContent).toContain(
    "Nothing is added until you've seen what it holds.",
  );
  await rig.click(rig.q('[data-testid="add-skill-folder"]'));
  expect(rig.transport.only("pick_skill")).toEqual([{ call: "pick_skill", files: false }]);
  expect(rig.skills.links.asked).toEqual([
    { preview_skill: { from: { path: "/Users/r/Skills/bail-applications" } } },
  ]);
  // nothing added yet
  expect(rig.names()).toEqual([]);
  await rig.click(rig.q('[data-testid="add-skill-add"]'));
  expect(rig.names()).toEqual(["bail-applications", "cheque-bounce", "court-fees"]);
  expect(rig.sources()).toEqual(Array(3).fill("Added on this computer"));
  expect(rig.skills.sent).toEqual([]);
});

test("a folder with no SKILL.md says so in the panel", async () => {
  rig = await mount([]);
  rig.skills.links.refuse.preview = "skill.no_skill_md";
  await rig.click(rig.q('[data-testid="add-skill-folder"]'));
  expect(rig.q('[data-testid="add-skill-trouble"]')?.textContent).toBe(
    "There's no SKILL.md file in it. Choose a folder or a zip that holds one.",
  );
});

test("a skills.sh name previews and adds with add_skills; the list is read again", async () => {
  rig = await mount([]);
  const input = rig.q<HTMLInputElement>('[data-testid="add-skill-field"]')!;
  await act(async () => {
    Reflect.set(HTMLInputElement.prototype, "value", "lexdesk/skills", input);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await rig.click(rig.q('[data-testid="add-skill-look"]'));
  expect(rig.q('[data-testid="add-skill-preview"]')?.textContent).toContain(
    "Use them inEvery matter",
  );
  await rig.click(rig.q('[data-testid="add-skill-add"]'));
  expect(rig.skills.links.asked.at(-1)).toEqual({
    add_skills: {
      token: "t1",
      pick: ["bail-applications", "cheque-bounce", "court-fees"],
      scope: "all",
    },
  });
  expect(rig.names()).toEqual(["bail-applications", "cheque-bounce", "court-fees"]);
  expect(rig.skills.sent).toEqual([]);
});

test("Choose a zip previews the zip, not add_skill", async () => {
  rig = await mount([]);
  await rig.click(rig.q('[data-testid="add-skill-zip"]'));
  expect(rig.skills.links.asked).toEqual([
    { preview_skill: { from: { path: "/Users/r/Downloads/lease-review.zip" } } },
  ]);
  expect(rig.skills.sent).toEqual([]);
});

test("Cancel in the picker sends nothing", async () => {
  rig = await mount([]);
  rig.skills.pick = null;
  await rig.click(rig.q('[data-testid="add-skill-folder"]'));
  expect(rig.skills.links.asked).toEqual([]);
  expect(rig.q('[data-testid="add-skill-trouble"]')).toBeNull();
});

test("a refused switch says one line under Add a skill, and the next change clears it", async () => {
  rig = await mount(PACKS);
  rig.skills.refuseNext = "skill.invalid";
  const first = () => rig!.pane().querySelector<HTMLElement>('[role="switch"]');
  await rig.click(first());
  expect(rig.q('[data-testid="skills-failed"]')?.textContent).toBe(
    "Clauseo couldn't make that change. Your skills are as they were.",
  );
  await rig.click(first());
  expect(rig.q('[data-testid="skills-failed"]')).toBeNull();
});

test("settings_changed reads the list again", async () => {
  rig = await mount([]);
  rig.skills.packs.push(pack("x", "From elsewhere"));
  await act(async () => rig!.transport.settingsChanged(SETTINGS));
  await rig.settle();
  expect(rig.names()).toEqual(["From elsewhere"]);
});

test("no picker, no Skills", () => {
  expect(skillsShown(new FakeTransport({}))).toBe(false);
  expect(skillsShown(new FakeTransport({ pickSkill: () => null }))).toBe(true);
});
