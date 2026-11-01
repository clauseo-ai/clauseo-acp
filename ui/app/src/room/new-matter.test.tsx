// New matter from the rail, over FakeTransport: an existing folder through Change, a new one in the parent the picker
// gives, H20's default location with a NextHost, nothing sent on a cancelled picker, and the refusal line for each
// code, never the host's message.
// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { createErrorLine } from "@clauseo/components/clauseo/matter-form-check.ts";
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
const NAME = "Mehta v. Mehta";

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

/** New matter, pressed, and the name typed. */
async function openForm(name = NAME) {
  await act(async () => byId("new-matter")?.click());
  await settle();
  const field = byId<HTMLInputElement>("matter-name");
  expect(field).not.toBeNull();
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

test("an existing folder through Change: its display shows, create_matter takes it, the new matter opens and focus goes back", async () => {
  const transport = await mount({
    ...today(),
    pickFolder: () => ({ folder: "ref-existing", display: "~/Documents/Mehta" }),
  });
  await openForm();
  // before a pick, no folder words: Change alone
  expect(dialog()?.textContent).not.toContain("Documents");
  await press("matter-existing");
  expect(dialog()?.textContent).toContain("~/Documents/Mehta");
  await press("create-matter");
  expect(created(transport)).toEqual([
    { create_matter: { name: NAME, source: { existing_folder: { folder: "ref-existing" } } } },
  ]);
  expect(transport.only("pick_folder")).toHaveLength(1);
  expect(title()).toBe(NAME);
  expect(dialog()).toBeNull();
  expect(document.activeElement).toBe(byId("new-matter"));
});

test("a new folder: Create the matter asks for the parent and sends new_folder", async () => {
  const transport = await mount({
    ...today(),
    pickFolder: () => ({ folder: "ref-parent", display: "~/Documents" }),
  });
  await openForm();
  await press("create-matter");
  expect(transport.only("pick_folder")).toHaveLength(1);
  expect(created(transport)).toEqual([
    { create_matter: { name: NAME, source: { new_folder: { folder: "ref-parent" } } } },
  ]);
  expect(title()).toBe(NAME);
});

test("a cancelled picker sends nothing: Change keeps the form as it was, Create leaves it open", async () => {
  const transport = await mount({ ...today(), pickFolder: () => null });
  await openForm();
  await press("matter-existing");
  expect(dialog()?.textContent).not.toContain("Documents");
  await press("create-matter");
  expect(transport.only("pick_folder")).toHaveLength(2);
  expect(created(transport)).toEqual([]);
  expect(dialog()).not.toBeNull();
  expect(byId<HTMLInputElement>("matter-name")?.value).toBe(NAME);
});

test("with a NextHost: the default location's folder line, create_matter with default_location and no picker, client and next date", async () => {
  let list = [detailed(A)];
  const sent: NextCommand[] = [];
  const transport = await mount({
    next: fakeNext({
      matters: () => list,
      location: "~/Documents/Clauseo",
      command: (c): NextOutcome | undefined => {
        sent.push(c);
        if (!("create_matter" in c)) return undefined;
        list = [...list, detailed({ matter: MADE, name: c.create_matter.name, kind: "folder" })];
        return { matter_created: { matter: MADE } };
      },
    }),
  });
  await openForm();
  expect(dialog()?.textContent).toContain(`~/Documents/Clauseo/${NAME}`);
  const fields = [...(dialog()?.querySelectorAll<HTMLInputElement>("input") ?? [])];
  // name, client, next date: H5's fields show
  expect(fields).toHaveLength(3);
  await act(async () => type(fields[1] as HTMLInputElement, "Asha Rao"));
  await act(async () => type(fields[2] as HTMLInputElement, "30 November 2026"));
  await press("create-matter");
  expect(transport.only("pick_folder")).toEqual([]);
  expect(sent).toContainEqual({
    create_matter: {
      name: NAME,
      source: "default_location",
      client: "Asha Rao",
      next_date: "2026-11-30",
    },
  });
  expect(transport.only("dispatch")).toEqual([]);
  expect(title()).toBe(NAME);
});

test("without a NextHost the form asks for no client or next date", async () => {
  await mount(today());
  await openForm();
  expect(dialog()?.querySelectorAll("input")).toHaveLength(1);
});

const CODES = [
  "matter.folder_refused",
  "matter.folder_is_drive",
  "matter.folder_is_home",
  "matter.folder_is_system",
  "matter.folder_unreadable",
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

test("the refusal lines differ by reason: a drive is not a home folder", () => {
  expect(createErrorLine("en", "matter.folder_is_drive", "C:\\")).not.toBe(
    createErrorLine("en", "matter.folder_is_home", "C:\\"),
  );
});
