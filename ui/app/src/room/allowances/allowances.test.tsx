// @vitest-environment happy-dom
// R18 in the room over FakeTransport (fake-allowances.ts): the open matter's menu sends "Switch off
// the Safe workspace…" as request_escalation, becomes "Switch the Safe workspace back on" once the
// matter is switched off and sends switch_protection_on; switch-off hides while the firm requires the
// Safe workspace or forbids working without it; each refusal's words on the row's note line; "Use
// files from another matter…" lists the other matters and sends the pick; and the protection note's
// "Remembered for this matter" with Forget, "Switch it back on" and its line about the next start.
import { aw } from "@clauseo/components/lib/words-allowances.ts";
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { settled } from "@clauseo/components/lib/late.ts";
import { act } from "react";
import { afterEach, beforeEach, expect, test } from "vitest";
import { MATTER, uuid } from "../../testing/stream-script.ts";
import { fakeAllowances, type FakeAllowancesRefusals } from "../../transport/fake-allowances.ts";
import type {
  MatterAllowance,
  MatterSummary,
  Protection,
  ProtectionView,
} from "../../transport/protocol.ts";
import { PROT, view } from "../protection/fixtures.ts";
import { defaultOutcome, mountRoom, THE_MATTER, type Harness } from "../thread/harness.ts";

// older than MATTER, so MATTER (the newest) is the one the room opens
const OTHER: MatterSummary = { matter: uuid(0, 2), name: "Rao v. Union of India", kind: "folder" };
const KEPT: MatterAllowance[] = [
  {
    matter: MATTER,
    allowance: uuid(1, 8),
    target: { folder: { path: "/Users/asha/Desktop/Exhibits" } },
    mode: "read_only",
    allowed: true,
    at: 0,
  },
  {
    matter: MATTER,
    allowance: uuid(2, 8),
    target: { site: { host: "files-share.example" } },
    mode: "read_only",
    allowed: false,
    at: 0,
  },
];

let h: Harness | null = null;
beforeEach(() =>
  localStorage.setItem(
    "clauseo.protection-explained",
    JSON.stringify({ computer: true, matters: [MATTER] }),
  ),
);
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
  localStorage.clear();
});

async function mount(
  protection: Protection,
  {
    refuse = {},
    over = {},
    settings = false,
    otherName = OTHER.name,
  }: {
    refuse?: FakeAllowancesRefusals;
    over?: Partial<ProtectionView>;
    settings?: boolean;
    otherName?: string;
  } = {},
) {
  const allow = fakeAllowances({ allowances: KEPT, refuse });
  h = await mountRoom({
    matters: [
      { ...THE_MATTER, protection },
      { ...OTHER, name: otherName, protection: PROT.a },
    ],
    next: { parts: ["app"], query: () => ({ threads: [] }) },
    protection: { view: () => view(protection, over) },
    dispatch: (command) => (allow.command(command) ? "done" : defaultOutcome(command)),
    ...(settings ? { openSettings: () => {} } : {}),
  });
  allow.bind(h.transport);
  await settle();
  return allow;
}

const settle = async () => {
  for (let i = 0; i < 5; i += 1) {
    await act(async () => settled());
    await h!.settle();
  }
};
const items = () => [...document.querySelectorAll<HTMLElement>('[role="menuitem"]')];
const item = (text: string) => items().find((i) => i.textContent?.includes(text)) ?? null;
const openMenu = async () =>
  h!.click(h!.host.querySelector(`[aria-label="Actions for ${THE_MATTER.name}"]`));
const press = async (el: HTMLElement | null) => {
  await h!.click(el);
  await settle();
};
const row = () => h!.all("matter-row").find((r) => r.textContent?.includes(THE_MATTER.name));
const OFF = aw("en", "Switch off the Safe workspace…");
const ON = aw("en", "Switch the Safe workspace back on");
const FILES = aw("en", "Use files from another matter…");

test("Switch off the Safe workspace… sends request_escalation, and once off the item says back on and sends switch_protection_on", async () => {
  const allow = await mount(PROT.a);
  await openMenu();
  expect(item(ON)).toBeNull();
  await press(item(OFF));
  expect(h!.sent("request_escalation")).toEqual([
    { request: { switch_protection_off: { matter: MATTER } } },
  ]);
  expect(allow.isOff(MATTER)).toBe(true);
  await settle();
  await openMenu();
  expect(item(OFF)).toBeNull();
  await press(item(ON));
  expect(h!.sent("switch_protection_on")).toEqual([{ matter: MATTER }]);
  expect(allow.isOff(MATTER)).toBe(false);
});

test.each([
  ["requires the Safe workspace", { required: true }],
  ["forbids working without it", { unprotected_auto_allowed: false }],
] as const)("switch-off hides while the firm %s", async (_, over) => {
  await mount(PROT.a, { over });
  await openMenu();
  expect(item(OFF)).toBeNull();
  // the other item stays
  expect(item(FILES)).not.toBeNull();
});

test.each([
  [
    "sandbox.required",
    aw("en", "Your firm requires the Safe workspace in every matter, so it can't be switched off."),
  ],
  ["sandbox.unprotected_auto_forbidden", errorWords("en", "sandbox.unprotected_auto_forbidden")],
] as const)(
  "a switch-off refused with %s says its line on the row's note line",
  async (code, line) => {
    await mount(PROT.a, { refuse: { switch_protection_off: code } });
    await openMenu();
    await press(item(OFF));
    expect(row()?.textContent).toContain(line);
    // never the start-up words
    expect(row()?.textContent).not.toContain(errorWords("en", "sandbox.required"));
  },
);

test("Use files from another matter… lists the other matters, and a pick sends files_from_other_matter", async () => {
  await mount(PROT.a);
  await openMenu();
  await press(item(FILES));
  const dialog = document.querySelector<HTMLElement>('[data-testid="other-matter"]');
  const picks = [...document.querySelectorAll<HTMLElement>('[data-testid="other-matter-pick"]')];
  expect(dialog?.textContent).toContain(aw("en", "Use files from another matter"));
  expect(picks.map((p) => p.textContent)).toEqual([`\u2068${OTHER.name}\u2069`]);
  await press(picks[0] ?? null);
  expect(h!.sent("request_escalation")).toEqual([
    { request: { files_from_other_matter: { matter: MATTER, from: OTHER.matter } } },
  ]);
  expect(document.querySelector('[data-testid="other-matter"]')).toBeNull();
});

/** Opens the note from the composer's line, and plays the matter's header with its answers. */
async function openNote(allowances: MatterAllowance[] = KEPT) {
  await press(h!.one("workspace-line"));
  h!.deliverOn(`matter:${MATTER}`, [
    {
      reset: {
        view_window: {
          header: {
            matter_header: {
              order: 0,
              matter: MATTER,
              name: THE_MATTER.name,
              kind: "folder",
              allowances,
              switched_off: true,
            },
          },
          entries: [],
          has_older: false,
        },
      },
    },
  ]);
  await settle();
  return document.body.querySelector<HTMLElement>('[data-testid="np-note"]');
}

test("the switched-off note lists the matter's remembered answers, Forget sends forget_allowance, and Switch it back on sends switch_protection_on", async () => {
  await mount(PROT.off);
  const note = await openNote();
  expect(note?.textContent).toContain(
    aw("en", "It applies the next time Clauseo starts work in this matter."),
  );
  const list = note?.querySelector('[data-testid="np-remembered"]');
  expect(list?.textContent).toContain(aw("en", "Remembered for this matter"));
  const rows = [...(list?.querySelectorAll('[data-testid="remembered-row"]') ?? [])];
  expect(rows.map((r) => r.textContent?.replace(/[\u2068\u2069]/g, ""))).toEqual([
    "/Users/asha/Desktop/ExhibitsFolder, read only · Allowed",
    "files-share.exampleSite · Not allowed",
  ]);
  // the folder and the site are bidi-isolated
  expect(rows[0]?.querySelector("bdi")?.textContent).toBe(
    "\u2068/Users/asha/Desktop/Exhibits\u2069",
  );
  await press(
    note?.querySelector<HTMLElement>(
      'button[aria-label="Forget \u2068files-share.example\u2069"]',
    ) ?? null,
  );
  expect(h!.sent("forget_allowance")).toEqual([{ matter: MATTER, allowance: uuid(2, 8) }]);
  expect(note?.querySelectorAll('[data-testid="remembered-row"]').length).toBe(1);
  await press(note?.querySelector<HTMLElement>('[data-testid="protection-switch-on"]') ?? null);
  expect(h!.sent("switch_protection_on")).toEqual([{ matter: MATTER }]);
});

test("a refused Forget in the note says its words", async () => {
  await mount(PROT.off, { refuse: { forget_allowance: "internal" } });
  const note = await openNote();
  await press(note?.querySelector<HTMLElement>('[data-testid="remembered-forget"]') ?? null);
  expect(note?.querySelector('[data-testid="protection-failed"]')?.textContent).toBe(
    errorWords("en", "internal"),
  );
  expect(note?.querySelectorAll('[data-testid="remembered-row"]').length).toBe(2);
});

test("Remembered answers… shows once a protected matter has any, and opens Settings at Remembered answers", async () => {
  await mount(PROT.a, { settings: true });
  const SHOW = aw("en", "Remembered answers…");
  await openMenu();
  // the menu holds the matter's stream while it is open; no answers yet, no item
  expect(h!.transport.only("subscribe").map((c) => c.stream)).toContain(`matter:${MATTER}`);
  expect(item(SHOW)).toBeNull();
  h!.deliverOn(`matter:${MATTER}`, [
    {
      reset: {
        view_window: {
          header: {
            matter_header: {
              order: 0,
              matter: MATTER,
              name: THE_MATTER.name,
              kind: "folder",
              allowances: KEPT,
            },
          },
          entries: [],
          has_older: false,
        },
      },
    },
  ]);
  await settle();
  await press(item(SHOW));
  // SettingsSection::RememberedAnswers, which the window opens at its #allowances section
  expect(h!.transport.only("open_settings")).toEqual([
    { call: "open_settings", section: "remembered_answers" },
  ]);
});

test("a remembered path with a right-to-left override and a line break shows as one isolated run, with neither", async () => {
  await mount(PROT.off);
  const note = await openNote([
    { ...KEPT[0]!, target: { folder: { path: "/Users/asha/\u202Efdp.exe\nFake line" } } },
  ]);
  const text = note?.querySelector('[data-testid="remembered-row"] bdi')?.textContent ?? "";
  expect(text).toBe("\u2068/Users/asha/fdp.exeFake line\u2069");
  expect(text).not.toMatch(/[\u202E\n]/);
});

test("a matter name with a right-to-left override shows isolated, without it, in the other-matters list", async () => {
  await mount(PROT.a, { otherName: "Rao \u202Ev. Union" });
  await openMenu();
  await press(item(FILES));
  const pick = document.querySelector<HTMLElement>('[data-testid="other-matter-pick"]');
  expect(pick?.textContent).toBe("\u2068Rao v. Union\u2069");
  expect(pick?.textContent).not.toContain("\u202E");
});

test("a pick refused with escalation.network_location says its words on the row's note line", async () => {
  await mount(PROT.a, { refuse: { files_from_other_matter: "escalation.network_location" } });
  await openMenu();
  await press(item(FILES));
  await press(document.querySelector<HTMLElement>('[data-testid="other-matter-pick"]'));
  expect(h!.sent("request_escalation")).toEqual([
    { request: { files_from_other_matter: { matter: MATTER, from: OTHER.matter } } },
  ]);
  expect(row()?.textContent).toContain(errorWords("en", "escalation.network_location"));
});
