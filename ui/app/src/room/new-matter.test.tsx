// New matter from the rail, over FakeTransport: the question first (New folder or Existing folder); Existing folder's
// picker at once, the name filled with the folder's own, nothing sent on a cancelled picker; New folder under H20's
// default location with a NextHost, or under the folder Change… picks; Back; and the refusal line for each code,
// never the host's message, with its one way on.
// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { createErrorLine } from "@clauseo/components/clauseo/matter-form-check.ts";
import { settled } from "@clauseo/components/lib/late.ts";
import { afterEach, expect, test } from "vitest";
import type { CommandEnvelope, CommandOutcome, MatterSummary } from "../transport/protocol.ts";
import { FakeTransport, type FakeHost } from "../transport/fake-transport.ts";
import { uuid } from "../testing/stream-script.ts";
import { detailed, fakeNext } from "./fake-next.ts";
import type { NextCommand, NextHostError, NextOutcome } from "./host-next.ts";
import { Room } from "./room.tsx";
import { connectRoom } from "./start.ts";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement | null = null;
let root: Root | null = null;

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  host = null;
});

const A: MatterSummary = { matter: uuid(1, 2), name: "Acme Ltd v. State", kind: "folder" };
const MADE = uuid(5, 2);
const NAME = "Mehta family trust";

/** Today's host: list_matters, and create_matter making MADE, or refusing with `refuse`. */
function today(refuse?: NextHostError): FakeHost {
  let list: MatterSummary[] = [A];
  return {
    query: () => ({ matters: list }),
    dispatch: (command: CommandEnvelope): CommandOutcome => {
      if (refuse !== undefined) throw refuse;
      if (!("create_matter" in command.command)) return "done";
      list = [A, { matter: MADE, name: command.command.create_matter.name, kind: "folder" }];
      return { matter_created: { matter: MADE } };
    },
  };
}

async function mount(h: FakeHost, language: "en" | "hi" = "en") {
  const transport = new FakeTransport(h);
  connectRoom(transport, { language });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root?.render(<Room transport={transport} />));
  await settle();
  return transport;
}

async function settle() {
  for (let i = 0; i < 3; i += 1)
    await act(async () => new Promise((resolve) => setTimeout(resolve, 0)));
}

const byId = <E extends HTMLElement>(id: string) =>
  document.querySelector<E>(`[data-testid="${id}"]`);
const dialog = () => document.querySelector('[role="dialog"]');
const title = () => host?.querySelector('[data-testid="matter"]')?.textContent;

function type(input: HTMLInputElement, text: string) {
  Reflect.set(HTMLInputElement.prototype, "value", text, input);
  input.dispatchEvent(new Event("input", { bubbles: true }));
}

/** New matter, pressed: the question. The dialog's chunk loads on the press: settled waits for it. */
async function ask() {
  await act(async () => byId("new-matter")?.click());
  await act(async () => settled());
}

async function press(id: string) {
  await act(async () => byId(id)?.click());
  await settle();
}

async function typeName(name: string) {
  const field = byId<HTMLInputElement>("matter-name");
  await act(async () => type(field as HTMLInputElement, name));
}

/** New matter, then Existing folder: the picker, and the form with the folder picked, the name as the folder's. */
async function existing() {
  await ask();
  await press("matter-existing");
}

/** New matter, then New folder, and the name typed. */
async function fresh(name = NAME) {
  await ask();
  await press("matter-made");
  await typeName(name);
}

const created = (transport: FakeTransport) =>
  transport
    .only("dispatch")
    .map((d) => d.command.command)
    .filter((c) => "create_matter" in c);

test("New matter asks one thing first: New folder or Existing folder, each saying what it means for the files", async () => {
  const transport = await mount(today());
  await ask();
  expect(byId("matter-name")).toBeNull();
  expect(byId("matter-made")?.textContent).toBe(
    "New folderClauseo makes an empty folder for this matter. Add papers to it any time.",
  );
  expect(byId("matter-existing")?.textContent).toBe(
    "Existing folderUse a folder that already holds the papers. It stays where it is, and nothing is moved or copied.",
  );
  expect(transport.only("pick_folder")).toEqual([]);
});

test("Existing folder opens the picker at once; the name is the folder's, selected to type over; Create makes the matter there", async () => {
  const transport = await mount({
    ...today(),
    pickFolder: () => ({ folder: "ref-existing", display: "Documents/Mehta family trust" }),
  });
  await existing();
  expect(transport.only("pick_folder")).toHaveLength(1);
  const field = byId<HTMLInputElement>("matter-name");
  expect(field?.value).toBe(NAME);
  expect(document.activeElement).toBe(field);
  expect([field?.selectionStart, field?.selectionEnd]).toEqual([0, NAME.length]);
  expect(dialog()?.textContent).toContain("Documents › Mehta family trust");
  expect(dialog()?.textContent).toContain(
    "The folder stays where it is. Nothing is moved or copied.",
  );
  expect(byId("folder-change")?.textContent).toBe("Change…");
  expect(byId<HTMLButtonElement>("create-matter")?.disabled).toBe(false);
  await press("create-matter");
  expect(created(transport)).toEqual([
    { create_matter: { name: NAME, source: { existing_folder: { folder: "ref-existing" } } } },
  ]);
  expect(transport.only("pick_folder")).toHaveLength(1);
  expect(title()).toBe(NAME);
  expect(dialog()).toBeNull();
  expect(document.activeElement).toBe(byId("new-matter"));
});

test("a name typed over the folder's goes to create_matter; the folder keeps its own", async () => {
  const transport = await mount({
    ...today(),
    pickFolder: () => ({ folder: "ref-existing", display: "Documents/Health Insurance" }),
  });
  await existing();
  await typeName("Rao health insurance claim");
  await press("create-matter");
  expect(created(transport)).toEqual([
    {
      create_matter: {
        name: "Rao health insurance claim",
        source: { existing_folder: { folder: "ref-existing" } },
      },
    },
  ]);
});

test("Back goes to the question, focus on the answer chosen, nothing sent", async () => {
  const transport = await mount({
    ...today(),
    pickFolder: () => ({ folder: "ref-existing", display: "Documents/Mehta" }),
  });
  await existing();
  const back = [...(dialog()?.querySelectorAll("button") ?? [])].find(
    (b) => b.textContent === "Back",
  );
  await act(async () => back?.click());
  await settle();
  expect(byId("matter-name")).toBeNull();
  expect(document.activeElement).toBe(byId("matter-existing"));
  expect(created(transport)).toEqual([]);
});

test("the dialog opens the first time New matter is pressed, once its chunk has loaded", async () => {
  await mount(today());
  expect(byId("matter-made")).toBeNull();
  await act(async () => byId("new-matter")?.click());
  await act(async () => settled());
  expect(dialog()).not.toBeNull();
  expect(byId("matter-made")).not.toBeNull();
});

test("a matter made from the folded rail folds it, and focus goes back to the sidebar's toggle", async () => {
  await mount({
    ...today(),
    pickFolder: () => ({ folder: "ref-existing", display: "Documents/Mehta" }),
  });
  const rail = () => host?.querySelector("nav.room-rail");
  await press("rail-toggle");
  expect(rail()?.hasAttribute("data-open")).toBe(true);
  await existing();
  await typeName(NAME);
  await press("create-matter");
  expect(title()).toBe(NAME);
  expect(dialog()).toBeNull();
  expect(rail()?.hasAttribute("data-open")).toBe(false);
  expect(document.activeElement).toBe(byId("rail-toggle"));
});

test("a path with a right-to-left override is isolated: the folder line in <bdi>, the refusal line between FSI and PDI", async () => {
  const spoof = "Documents/a\u202Efdp.exe";
  const SPOOF_SHOWN = "Documents › a\u202Efdp.exe";
  const refusal: NextHostError = {
    code: "matter.folder_unreadable",
    message: "",
    retryable: false,
  };
  await mount({ ...today(refusal), pickFolder: () => ({ folder: "ref-x", display: spoof }) });
  await existing();
  const path = dialog()?.querySelector(".folder-path");
  expect(path?.tagName).toBe("BDI");
  expect(path?.textContent).toBe(SPOOF_SHOWN);
  await press("create-matter");
  expect(dialog()?.querySelector('[role="alert"]')?.textContent).toContain(
    `\u2068${SPOOF_SHOWN}\u2069`,
  );
});

test("a cancelled picker goes back to the two choices, focus still on Existing folder, and sends nothing", async () => {
  const transport = await mount({ ...today(), pickFolder: () => null });
  await ask();
  byId("matter-existing")?.focus();
  await press("matter-existing");
  expect(document.activeElement).toBe(byId("matter-existing"));
  expect(transport.only("pick_folder")).toHaveLength(1);
  expect(byId("matter-name")).toBeNull();
  expect(byId("matter-existing")).not.toBeNull();
  expect(byId("matter-made")).not.toBeNull();
  expect(created(transport)).toEqual([]);
  expect(dialog()).not.toBeNull();
});

test("with a NextHost: New folder makes it in the default location, and create_matter takes it with the client, no picker", async () => {
  let list = [detailed(A)];
  const sent: NextCommand[] = [];
  const transport = await mount({
    next: fakeNext({
      matters: () => list,
      location: "Documents/Clauseo",
      command: (c): NextOutcome | undefined => {
        sent.push(c);
        if (!("create_matter" in c)) return undefined;
        list = [...list, detailed({ matter: MADE, name: c.create_matter.name, kind: "folder" })];
        return { matter_created: { matter: MADE } };
      },
    }),
  });
  await fresh();
  expect(document.activeElement).toBe(byId("matter-name"));
  expect(dialog()?.textContent).toContain(`Documents › Clauseo › ${NAME}`);
  expect(dialog()?.textContent).toContain(
    "Clauseo creates this folder. Add papers to it any time.",
  );
  const fields = [...(dialog()?.querySelectorAll<HTMLInputElement>("input") ?? [])];
  // name and client: no next date, no other field
  expect(fields).toHaveLength(2);
  await act(async () => type(fields[1] as HTMLInputElement, "Asha Rao"));
  await press("create-matter");
  expect(transport.only("pick_folder")).toEqual([]);
  expect(sent).toContainEqual({
    create_matter: { name: NAME, source: "default_location", client: "Asha Rao" },
  });
  // nothing but the rail's morning sweep (U16b), sent at launch whenever the clock reads 05:00 or later
  expect(transport.only("dispatch").filter((c) => !("sweep_done" in c.command.command))).toEqual(
    [],
  );
  expect(title()).toBe(NAME);
});

test("without a default location, New folder asks where to make it, and create_matter makes it in the folder picked", async () => {
  const transport = await mount({
    ...today(),
    pickFolder: () => ({ folder: "ref-cases", display: "Documents/Cases" }),
  });
  await fresh();
  // no client without a NextHost
  expect(dialog()?.querySelectorAll("input")).toHaveLength(1);
  expect(dialog()?.textContent).toContain("Choose where it goes.");
  expect(byId<HTMLButtonElement>("create-matter")?.disabled).toBe(true);
  await press("folder-change");
  expect(dialog()?.textContent).toContain(`Documents › Cases › ${NAME}`);
  await press("create-matter");
  expect(created(transport)).toEqual([
    { create_matter: { name: NAME, source: { new_folder: { folder: "ref-cases" } } } },
  ]);
  expect(title()).toBe(NAME);
});

test("with a NextHost, Change… makes the new folder in the folder picked instead, and its refusal names that folder", async () => {
  const sent: NextCommand[] = [];
  const refusal: NextHostError = {
    code: "matter.folder_unreadable",
    message: "",
    retryable: false,
  };
  await mount({
    ...refusingNext(refusal, sent),
    pickFolder: () => ({ folder: "ref-cases", display: "Documents/Cases" }),
  });
  await fresh();
  await press("folder-change");
  expect(dialog()?.textContent).toContain(`Documents › Cases › ${NAME}`);
  await press("create-matter");
  expect(sent).toContainEqual({
    create_matter: { name: NAME, source: { new_folder: { folder: "ref-cases" } } },
  });
  expect(dialog()?.querySelector('[role="alert"]')?.textContent).toBe(
    createErrorLine("en", "matter.folder_unreadable", "Documents/Cases"),
  );
});

const CODES = [
  "matter.folder_refused",
  "matter.folder_is_drive",
  "matter.folder_is_home",
  "matter.folder_is_system",
  "matter.folder_unreadable",
  "matter.folder_is_matter",
  "matter.folder_in_matters_root",
  "matter.folder_in_clauseo_data",
  "matter.folder_inside_matter",
  "matter.folder_holds_matter",
  "matter.folder_too_deep",
  "matter.folder_missing",
  "internal",
] as const;

for (const lang of ["en", "hi"] as const) {
  test(`each refusal says its line in the form, never the host's message (${lang})`, async () => {
    for (const code of CODES) {
      const refusal: NextHostError = { code, message: "SECRET-DETAIL for logs", retryable: false };
      const transport = await mount(
        { ...today(refusal), pickFolder: () => ({ folder: "ref", display: "/Volumes/Case" }) },
        lang,
      );
      await existing();
      await typeName(NAME);
      await press("create-matter");
      const alert = dialog()?.querySelector('[role="alert"]');
      expect(alert?.textContent, code).toBe(createErrorLine(lang, code, "/Volumes/Case"));
      expect(dialog()?.textContent, code).not.toContain("SECRET-DETAIL");
      // still open, with the name as typed
      expect(byId<HTMLInputElement>("matter-name")?.value).toBe(NAME);
      expect(created(transport)).toHaveLength(1);
      await act(async () => root?.unmount());
      host?.remove();
    }
    root = null;
    host = null;
  });
}

/** A NextHost whose create_matter is refused with `refusal`, and whose commands are recorded in `sent`. `open` is the
 * matter opened last, so the one the form names is not the open one. */
function refusingNext(refusal: NextHostError, sent: NextCommand[], open?: MatterSummary): FakeHost {
  const matters = [
    detailed(A),
    ...(open === undefined ? [] : [detailed(open, { last_opened: 1 })]),
  ];
  return {
    next: fakeNext({
      matters: () => matters,
      location: "Documents/Clauseo",
      command: (c): NextOutcome | undefined => {
        sent.push(c);
        if ("create_matter" in c) throw refusal;
        return undefined;
      },
    }),
  };
}

const NESTED = [
  ["matter.folder_inside_matter", "This folder is inside the folder of {name}."],
  ["matter.folder_holds_matter", "This folder holds the folder of {name}."],
] as const;

for (const [code, words] of NESTED) {
  test(`${code} names the matter on the list, and Open {name} sends open_matter and closes the form`, async () => {
    const sent: NextCommand[] = [];
    const OPEN: MatterSummary = {
      matter: uuid(3, 2),
      name: "Rao v. Union of India",
      kind: "folder",
    };
    const transport = await mount(
      refusingNext(
        { code, message: "SECRET-DETAIL", retryable: false, matter: A.matter },
        sent,
        OPEN,
      ),
    );
    expect(title()).toBe(OPEN.name);
    await fresh();
    await press("create-matter");
    expect(dialog()?.querySelector('[role="alert"]')?.textContent).toBe(
      words.replace("{name}", `\u2068${A.name}\u2069`),
    );
    expect(dialog()?.textContent).not.toContain("SECRET-DETAIL");
    expect(byId("open-matter")?.textContent).toBe(`Open \u2068${A.name}\u2069`);
    // a new folder's refusal may turn on its name: Create stays free
    expect(byId<HTMLButtonElement>("create-matter")?.disabled).toBe(false);
    await press("open-matter");
    expect(sent).toContainEqual({ open_matter: { matter: A.matter } });
    expect(created(transport)).toEqual([]);
    expect(dialog()).toBeNull();
    expect(title()).toBe(A.name);
  });
}

const UNNAMED: [string, NextHostError][] = [
  [
    "a nested folder whose matter is not on the list",
    { code: "matter.folder_inside_matter", message: "", retryable: false, matter: uuid(9, 9) },
  ],
  [
    "a folder holding a matter being made or forgotten (no matter)",
    { code: "matter.folder_holds_matter", message: "", retryable: false },
  ],
  [
    "folder_is_matter, even naming a matter on the list",
    { code: "matter.folder_is_matter", message: "", retryable: false, matter: A.matter },
  ],
];

for (const [what, refusal] of UNNAMED) {
  test(`${what} says the refused line, with no Open, and Choose another folder`, async () => {
    const sent: NextCommand[] = [];
    await mount(refusingNext(refusal, sent));
    await fresh();
    await press("create-matter");
    expect(dialog()?.querySelector('[role="alert"]')?.textContent).toBe(
      "Clauseo can't make a matter of this folder. Choose another folder.",
    );
    expect(dialog()?.textContent).not.toMatch(/already a matter/);
    expect(byId("open-matter")).toBeNull();
    expect(byId("folder-choose-another")).not.toBeNull();
    expect(byId<HTMLButtonElement>("create-matter")?.disabled).toBe(false);
    expect(sent.filter((c) => "open_matter" in c)).toEqual([]);
    expect(dialog()).not.toBeNull();
  });
}

test("the matters' own folder is a choose-another refusal, and says where Clauseo keeps matters with its path", async () => {
  const refusal: NextHostError = {
    code: "matter.folder_in_matters_root",
    message: "",
    retryable: false,
  };
  const transport = await mount({
    ...today(refusal),
    pickFolder: () => ({ folder: "ref-root", display: "Documents/Clauseo" }),
  });
  await existing();
  await press("create-matter");
  expect(dialog()?.querySelector('[role="alert"]')?.textContent).toBe(
    "\u2068Documents › Clauseo\u2069 is the folder Clauseo keeps matters in. Choose the folder for one matter.",
  );
  expect(byId("open-matter")).toBeNull();
  // its one way on: another folder, through the picker
  await press("folder-choose-another");
  expect(transport.only("pick_folder")).toHaveLength(2);
});

test("the refusal lines differ by reason: a drive is not a home folder", () => {
  expect(createErrorLine("en", "matter.folder_is_drive", "C:\\")).not.toBe(
    createErrorLine("en", "matter.folder_is_home", "C:\\"),
  );
});

test("a folder refused holds Create back until another is chosen, which drops the line; another failure leaves Create to try again", async () => {
  let picks = 0;
  const refusal: NextHostError = {
    code: "matter.folder_unreadable",
    message: "",
    retryable: false,
  };
  const transport = await mount({
    ...today(refusal),
    pickFolder: () => {
      picks += 1;
      return { folder: `ref-${picks}`, display: `Documents/Folder ${picks}` };
    },
  });
  await existing();
  await press("create-matter");
  expect(dialog()?.querySelector('[role="alert"]')?.textContent).toContain(
    "\u2068Documents › Folder 1\u2069",
  );
  expect(byId<HTMLButtonElement>("create-matter")?.disabled).toBe(true);
  await press("folder-choose-another");
  expect(dialog()?.querySelector('[role="alert"]')).toBeNull();
  expect(dialog()?.textContent).toContain("Documents › Folder 2");
  expect(byId<HTMLButtonElement>("create-matter")?.disabled).toBe(false);
  expect(created(transport)).toHaveLength(1);
});

test("a create that fails for another reason says so, and Create stays free to try again", async () => {
  const refusal: NextHostError = { code: "internal", message: "", retryable: true };
  await mount({
    ...today(refusal),
    pickFolder: () => ({ folder: "ref", display: "Documents/Acme" }),
  });
  await existing();
  await press("create-matter");
  expect(dialog()?.querySelector('[role="alert"]')?.textContent).toBe(
    "Clauseo couldn't create the matter. Try again, or choose another folder.",
  );
  expect(byId("folder-choose-another")).toBeNull();
  expect(byId<HTMLButtonElement>("create-matter")?.disabled).toBe(false);
});
