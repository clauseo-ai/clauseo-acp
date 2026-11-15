// The first screen's Open the folder for a folder New matter would warn about (folder rights, FR7 to FR10, FR13, FR14;
// first-folder-check.ts, first-folder-step.tsx): a folder picked on a network drive, or on a drive without file
// permissions, waits for the step, New matter's warning's words: Choose another folder, which picks again, or Open
// anyway, which makes it a matter. A fine folder, or a check that refuses, is made at once. And a folder a matter on the
// list already uses opens that matter, whether the host answers with it (MatterCreated) or refuses naming it
// (matter.folder_is_matter): nothing made, nothing renamed.
// @vitest-environment happy-dom
import { act } from "react";
import { HI } from "@clauseo/components/lib/words-folder.hi.ts";
import { expect, test } from "vitest";
import type { CommandOutcome, PickedFolderSupport } from "../../transport/protocol.ts";
import { A, byId, mountFirstRun, press, sent, settle, text } from "./rig.tsx";

const FIRST = { folder: "folder-ref-1", display: "Volumes/Chambers/Rathi Steels" };
const SECOND = { folder: "folder-ref-2", display: "Documents/Matters/Mehta v. Mehta" };
const DRIVE = "Claude asks before every change. ChatGPT can't work here.";
const WORDS = {
  network: {
    head: "On a network drive",
    info: "Clauseo can't keep Claude and ChatGPT inside a folder on a network drive.",
  },
  no_permissions: {
    head: "No file permissions",
    info: "Clauseo can't keep Claude and ChatGPT inside a folder on this drive. USB drives often don't keep permissions.",
  },
} as const;
const title = () => document.querySelector('[data-testid="matter"]')?.textContent;
const step = () => byId("first-folder-warning");

/** The first screen whose picker gives `picks` in turn, the folder check answering `checks` in turn. */
async function mount(
  checks: PickedFolderSupport[],
  picks: (typeof FIRST | null)[] = [FIRST, SECOND],
  language: "en" | "hi" = "en",
) {
  let asked = 0;
  const transport = await mountFirstRun({
    matters: [],
    language,
    folderSupport: () => checks[asked++] ?? {},
  });
  let n = 0;
  transport.pickFolder = async () => picks[n++] ?? null;
  return transport;
}

const created = (transport: Awaited<ReturnType<typeof mount>>) => sent(transport, "create_matter");
const sayIn = (lang: "en" | "hi", en: keyof typeof HI) => (lang === "en" ? en : HI[en]);

for (const lang of ["en", "hi"] as const) {
  for (const kind of ["network", "no_permissions"] as const) {
    test(`${kind}: the step says New matter's warning, and only Open anyway makes the matter (${lang})`, async () => {
      const transport = await mount([{ support: kind }], [FIRST], lang);
      await press(byId("open-folder"));
      expect(step()?.getAttribute("data-kind")).toBe(kind);
      const said = step()?.textContent ?? "";
      expect(said).toContain(sayIn(lang, WORDS[kind].head));
      expect(said).toContain(sayIn(lang, DRIVE));
      expect(said).toContain(sayIn(lang, WORDS[kind].info));
      expect(byId("first-folder-choose")?.textContent).toBe(sayIn(lang, "Choose another folder"));
      expect(byId("first-folder-anyway")?.textContent).toBe(sayIn(lang, "Open anyway"));
      expect(created(transport)).toEqual([]);
      expect(byId<HTMLButtonElement>("open-folder")?.disabled).toBe(false);
      await press(byId("first-folder-anyway"));
      expect(step()).toBeNull();
      expect(created(transport)).toEqual([
        { name: "Rathi Steels", source: { existing_folder: { folder: FIRST.folder } } },
      ]);
      expect(title()).toBe("Rathi Steels");
    });
  }
}

test("Choose another folder picks again, and a fine folder then is made at once", async () => {
  const transport = await mount([{ support: "network" }, {}]);
  await press(byId("open-folder"));
  await press(byId("first-folder-choose"));
  expect(step()).toBeNull();
  expect(created(transport)).toEqual([
    { name: "Mehta v. Mehta", source: { existing_folder: { folder: SECOND.folder } } },
  ]);
});

test("Choose another folder with the picker cancelled makes nothing", async () => {
  const transport = await mount([{ support: "no_permissions" }], [FIRST, null]);
  await press(byId("open-folder"));
  await press(byId("first-folder-choose"));
  expect(step()).toBeNull();
  expect(created(transport)).toEqual([]);
  expect(text()).not.toContain("Opening the folder…");
});

test("Escape takes the pick back: nothing made", async () => {
  const transport = await mount([{ support: "network" }], [FIRST]);
  await press(byId("open-folder"));
  await act(async () => {
    step()?.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
  });
  await settle();
  expect(step()).toBeNull();
  expect(created(transport)).toEqual([]);
});

test.each([
  ["a fine folder", () => ({})],
  ["a check that couldn't answer", () => ({ support: "check_failed" as const })],
  [
    "a check that refuses",
    () => {
      throw { code: "matter.folder_is_system", message: "", retryable: false };
    },
  ],
])("%s: made at once, with no step", async (_, check) => {
  const transport = await mountFirstRun({ matters: [], folderSupport: check });
  await press(byId("open-folder"));
  expect(step()).toBeNull();
  expect(created(transport).map((c) => c.name)).toEqual(["Rathi Steels"]);
});

test("a folder a matter on the list uses, answered with it (MatterCreated): that matter opens by its own name", async () => {
  const transport = await mountFirstRun({
    matters: [],
    folderSupport: () => ({}),
    create: () => ({ matter_created: { matter: A.matter } }),
    listAfter: [A],
  });
  await press(byId("open-folder"));
  expect(created(transport)).toHaveLength(1);
  expect(title()).toBe(A.name);
});

const IS_MATTER = {
  code: "matter.folder_is_matter",
  message: "",
  retryable: false,
  matter: A.matter,
};

test.each([
  ["the check passing it", () => ({})],
  // the check refuses it as the create does, once the host refuses a listed matter's folder (#1056)
  [
    "the check refusing it too",
    () => {
      throw IS_MATTER;
    },
  ],
])(
  "a folder a matter on the list uses, refused naming it (matter.folder_is_matter), %s: that matter opens, no refusal said",
  async (_, folderSupport) => {
    const transport = await mountFirstRun({
      matters: [],
      folderSupport,
      create: (): CommandOutcome => {
        throw IS_MATTER;
      },
      listAfter: [A],
    });
    await press(byId("open-folder"));
    expect(created(transport)).toHaveLength(1);
    expect(title()).toBe(A.name);
    expect(document.querySelector(".fl-refusal-head")).toBeNull();
    expect(step()).toBeNull();
  },
);
