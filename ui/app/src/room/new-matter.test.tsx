// New matter from the rail, over FakeTransport: an existing folder through Choose folder…, H20's default location
// offered as "Use a new folder in" with a NextHost, Create held back until there is a folder, nothing sent on a
// cancelled picker, and the refusal line for each code, never the host's message, with its one way on.
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

/** New matter, pressed, and the name typed. The dialog's chunk loads on the press: settled waits for it. */
async function openForm(name = NAME) {
  await act(async () => byId("new-matter")?.click());
  await act(async () => settled());
  const field = byId<HTMLInputElement>("matter-name");
  await act(async () => type(field as HTMLInputElement, name));
}

async function press(id: string) {
  await act(async () => byId(id)?.click());
  await settle();
}

const created = (transport: FakeTransport) =>
  transport
    .only("dispatch")
    .map((d) => d.command.command)
    .filter((c) => "create_matter" in c);

test("an existing folder through Choose folder…: its path shows, create_matter takes it, the new matter opens and focus goes back", async () => {
  const transport = await mount({
    ...today(),
    pickFolder: () => ({ folder: "ref-existing", display: "Documents/Mehta" }),
  });
  await openForm();
  // before a pick: no folder, and Create waits for one
  expect(dialog()?.textContent).toContain("No folder chosen");
  expect(byId<HTMLButtonElement>("create-matter")?.disabled).toBe(true);
  await press("matter-existing");
  expect(dialog()?.textContent).toContain("~/Documents/Mehta");
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

test("the dialog opens the first time New matter is pressed, once its chunk has loaded", async () => {
  await mount(today());
  expect(byId("matter-name")).toBeNull();
  await act(async () => byId("new-matter")?.click());
  await act(async () => settled());
  expect(dialog()).not.toBeNull();
  expect(byId<HTMLInputElement>("matter-name")).not.toBeNull();
});

test("a matter made from the folded rail folds it, and focus goes back to Matters", async () => {
  await mount({
    ...today(),
    pickFolder: () => ({ folder: "ref-existing", display: "Documents/Mehta" }),
  });
  const rail = () => host?.querySelector("nav.room-rail");
  await press("rail-toggle");
  expect(rail()?.hasAttribute("data-open")).toBe(true);
  await openForm();
  await press("matter-existing");
  await press("create-matter");
  expect(title()).toBe(NAME);
  expect(dialog()).toBeNull();
  expect(rail()?.hasAttribute("data-open")).toBe(false);
  expect(document.activeElement).toBe(byId("rail-toggle"));
});

test("a path with a right-to-left override is isolated: the folder line in <bdi>, the refusal line between FSI and PDI", async () => {
  const spoof = "Documents/a\u202Efdp.exe";
  const refusal: NextHostError = {
    code: "matter.folder_unreadable",
    message: "",
    retryable: false,
  };
  await mount({ ...today(refusal), pickFolder: () => ({ folder: "ref-x", display: spoof }) });
  await openForm();
  await press("matter-existing");
  const path = dialog()?.querySelector(".folder-path");
  expect(path?.tagName).toBe("BDI");
  expect(path?.textContent).toBe(`~/${spoof}`);
  await press("create-matter");
  expect(dialog()?.querySelector('[role="alert"]')?.textContent).toContain(
    `\u2068~/${spoof}\u2069`,
  );
});

test("a cancelled picker sends nothing: the form stays as it was, and Create still waits for a folder", async () => {
  const transport = await mount({ ...today(), pickFolder: () => null });
  await openForm();
  await press("matter-existing");
  expect(dialog()?.textContent).toContain("No folder chosen");
  await press("create-matter");
  expect(transport.only("pick_folder")).toHaveLength(1);
  expect(created(transport)).toEqual([]);
  expect(dialog()).not.toBeNull();
  expect(byId<HTMLInputElement>("matter-name")?.value).toBe(NAME);
});

test("with a NextHost: Use a new folder offers the default location, and create_matter takes it with the client, no picker", async () => {
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
  await openForm();
  // offered, never chosen for the lawyer
  expect(dialog()?.textContent).toContain("No folder chosen");
  expect(byId("matter-made")?.textContent).toBe(
    "Use a new folder in \u2068~/Documents/Clauseo\u2069",
  );
  await press("matter-made");
  expect(dialog()?.textContent).toContain(`~/Documents/Clauseo/${NAME}`);
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

test("without a NextHost the form asks for no client, and offers no new folder", async () => {
  await mount(today());
  await openForm();
  expect(dialog()?.querySelectorAll("input")).toHaveLength(1);
  expect(byId("matter-made")).toBeNull();
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
      await openForm();
      await press("matter-existing");
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
    await openForm();
    await press("matter-made");
    await press("create-matter");
    expect(dialog()?.querySelector('[role="alert"]')?.textContent).toBe(
      words.replace("{name}", `\u2068${A.name}\u2069`),
    );
    expect(dialog()?.textContent).not.toContain("SECRET-DETAIL");
    expect(byId("open-matter")?.textContent).toBe(`Open \u2068${A.name}\u2069`);
    // Create waits for another folder, as for any folder refused
    expect(byId<HTMLButtonElement>("create-matter")?.disabled).toBe(true);
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
  test(`${what} says the refused line, with no Open, and Create waits`, async () => {
    const sent: NextCommand[] = [];
    await mount(refusingNext(refusal, sent));
    await openForm();
    await press("matter-made");
    await press("create-matter");
    expect(dialog()?.querySelector('[role="alert"]')?.textContent).toBe(
      "Clauseo can't make a matter of this folder. Choose another folder.",
    );
    expect(dialog()?.textContent).not.toMatch(/already a matter/);
    expect(byId("open-matter")).toBeNull();
    expect(byId("folder-choose-another")).not.toBeNull();
    expect(byId<HTMLButtonElement>("create-matter")?.disabled).toBe(true);
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
  await openForm();
  await press("matter-existing");
  await press("create-matter");
  expect(dialog()?.querySelector('[role="alert"]')?.textContent).toBe(
    "\u2068~/Documents/Clauseo\u2069 is the folder Clauseo keeps matters in. Choose the folder for one matter.",
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
  await openForm();
  await press("matter-existing");
  await press("create-matter");
  expect(dialog()?.querySelector('[role="alert"]')?.textContent).toContain(
    "\u2068~/Documents/Folder 1\u2069",
  );
  expect(byId<HTMLButtonElement>("create-matter")?.disabled).toBe(true);
  await press("folder-choose-another");
  expect(dialog()?.querySelector('[role="alert"]')).toBeNull();
  expect(dialog()?.textContent).toContain("~/Documents/Folder 2");
  expect(byId<HTMLButtonElement>("create-matter")?.disabled).toBe(false);
  expect(created(transport)).toHaveLength(1);
});

test("a create that fails for another reason says so, and Create stays free to try again", async () => {
  const refusal: NextHostError = { code: "internal", message: "", retryable: true };
  await mount({
    ...today(refusal),
    pickFolder: () => ({ folder: "ref", display: "Documents/Acme" }),
  });
  await openForm();
  await press("matter-existing");
  await press("create-matter");
  expect(dialog()?.querySelector('[role="alert"]')?.textContent).toBe(
    "Clauseo couldn't create the matter. Try again, or choose another folder.",
  );
  expect(byId("folder-choose-another")).toBeNull();
  expect(byId<HTMLButtonElement>("create-matter")?.disabled).toBe(false);
});
