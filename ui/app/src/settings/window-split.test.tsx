// @vitest-environment happy-dom
// The window split (U15): Settings sends the connector commands; the room (the main window) sends only the hint's
// dismiss_case_law_hint and the focus's refresh_balance. Over the room as App mounts it, with every ConnectorStatus
// played, available and not, the fake host's dispatched list holds no connector command; over an open thread with
// the case-law hint and the out-of-credit note pressed in every state, it holds none but those two; no file under
// room/ or the room's entry imports the settings window's connector code, and only room/connectors/main-commands.ts
// dispatches a connector command; Settings' command type can't carry the room's dismiss_case_law_hint, and the room's
// can't carry a settings-only command. UC: the room sends only retry_protection; Settings also remove_sandbox_setup.
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, test } from "vitest";
import { detailed, fakeNext } from "../room/fake-next.ts";
import { Room } from "../room/room.tsx";
import { connectRoom } from "../room/start.ts";
import { stubFonts } from "../room/thread/testing.ts";
import { frameBatch } from "../store/batch.ts";
import { StoreContext } from "../store/hooks.ts";
import { createViewStore } from "../store/store.ts";
import { uuid } from "../testing/stream-script.ts";
import { FakeTransport } from "../transport/fake-transport.ts";
import type { ConnectorStatus, ConnectorsState } from "../transport/protocol.ts";
import type { MainConnectorCommand } from "../room/connectors/main-commands.ts";
import { mountRoom } from "../room/thread/harness.ts";
import { agentText, reset, step, THREAD, TURN, userMessage } from "../testing/stream-script.ts";
import type { SettingsConnectorCommand } from "./connector-commands.ts";
import type { RoomProtectionCommand } from "../room/protection/commands.ts";
import type { SettingsProtectionCommand } from "./sections/workspace-commands.ts";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
stubFonts();

let host: HTMLDivElement | null = null;
let root: Root | null = null;

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  host = null;
});

const STATUSES: ConnectorStatus[] = [
  "not_added",
  "signing_in",
  { signed_in: { low: false } },
  { signed_in: { low: true, balance: { credits: 180, paise: 1_800 } } },
  { out_of_credit: { pay_url: "https://data.clauseo.chat/pay/x" } },
  "signed_out",
  "key_revoked",
  { error: { message: "no" } },
];

const stateOf = (status: ConnectorStatus, available: boolean): ConnectorsState => ({
  available,
  locked: !available,
  connectors: [
    {
      id: "case-law",
      name: "Indian Kanoon",
      price_paise: 2_360,
      packs: [{ pack: "p200", credits: 200, paise: 2_360 }],
      status,
      pay_url: "https://data.clauseo.chat/pay/y",
    },
  ],
});

test("the room sends no connector command, whatever the connectors' state", async () => {
  const matter = { matter: uuid(1, 2), name: "Acme Ltd v. State", kind: "folder" as const };
  const transport = new FakeTransport({
    next: {
      ...fakeNext({ matters: () => [detailed(matter)] }),
      first: () => [{ connectors_changed: stateOf("not_added", true) }],
    },
  });
  connectRoom(transport, {});
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () =>
    root?.render(
      <StoreContext value={createViewStore(transport, { batch: () => frameBatch })}>
        <Room transport={transport} />
      </StoreContext>,
    ),
  );
  await act(async () => new Promise((resolve) => setTimeout(resolve, 0)));
  for (const available of [true, false])
    for (const status of STATUSES)
      await act(async () => transport.appEvent({ connectors_changed: stateOf(status, available) }));
  await act(async () => globalThis.dispatchEvent(new Event("focus")));
  expect(transport.connectorCommands()).toEqual([]);
});

test("with the hint and the note pressed in every state, the room sends only dismiss_case_law_hint and refresh_balance", async () => {
  const h = await mountRoom({
    next: { parts: ["app"], query: () => ({ threads: [] }) },
    openSettings: () => {},
  });
  h.deliver([
    reset([userMessage(1, 1, "Find judgments."), agentText(2, 1, "Found some."), step(3)]),
  ]);
  await h.settle();
  const press = async (id: string) => {
    for (const el of h.all(id)) await h.click(el);
  };
  for (const available of [true, false])
    for (const status of STATUSES) {
      await act(async () =>
        h.transport.appEvent({ connectors_changed: stateOf(status, available) }),
      );
      await act(async () =>
        h.transport.appEvent({ case_law_hint: { matter: uuid(1, 2), thread: THREAD, turn: TURN } }),
      );
      await h.settle();
      await press("case-law-set-up");
      await press("out-of-credit-add");
      await act(async () => globalThis.dispatchEvent(new Event("focus")));
      await press("case-law-dismiss");
      await press("out-of-credit-hide");
    }
  const kinds = new Set(h.transport.connectorCommands().map((c) => Object.keys(c)[0]));
  // the presses happened: the hint showed (not added, available) and the note showed (out of credit, available)
  expect(kinds).toEqual(new Set(["dismiss_case_law_hint", "refresh_balance"]));
  expect(h.transport.only("open_settings").every((c) => c.section === "connectors")).toBe(true);
  expect(h.transport.only("open_settings").length).toBeGreaterThan(0);
  await h.unmount();
});

test("no file under room/, nor the room's entry, imports the settings window's connector code", () => {
  const src = join(import.meta.dirname, "..");
  const roomFiles = readdirSync(join(src, "room"), { recursive: true })
    .map((f) => join(src, "room", String(f)))
    .filter((f) => /\.(ts|tsx)$/.test(f));
  expect(roomFiles.length).toBeGreaterThan(50);
  for (const file of [...roomFiles, join(src, "main.tsx"), join(src, "App.tsx")]) {
    const text = readFileSync(file, "utf8");
    expect(text, file).not.toMatch(/settings\/sections\/connectors/);
    expect(text, file).not.toMatch(/settings\/connector-commands/);
  }
});

test("only room/connectors/main-commands.ts dispatches a connector command from the room", () => {
  const src = join(import.meta.dirname, "..");
  const senders = readdirSync(join(src, "room"), { recursive: true })
    .map((f) => join(src, "room", String(f)))
    .filter((f) => /\.(ts|tsx)$/.test(f) && !/\.test\.tsx?$/.test(f))
    .filter((f) => /command: \{ connector:|\bconnector: \{/.test(readFileSync(f, "utf8")));
  expect(senders.map((f) => f.slice(src.length + 1))).toEqual(["room/connectors/main-commands.ts"]);
});

test("the room's command type has no settings-only command", () => {
  const ok: MainConnectorCommand[] = [
    { dismiss_case_law_hint: { matter: uuid(1, 2) } },
    { refresh_balance: { id: "case-law" } },
  ];
  // @ts-expect-error: Settings' command, which the host refuses from the main window
  const add: MainConnectorCommand = { add: { id: "case-law" } };
  // @ts-expect-error: as is open_pay_link
  const pay: MainConnectorCommand = { open_pay_link: { id: "case-law" } };
  expect([...ok, add, pay]).toHaveLength(4);
});

test("Settings' command type has no dismiss_case_law_hint", () => {
  const ok: SettingsConnectorCommand = { add: { id: "case-law" } };
  // @ts-expect-error: the room's command, which the host refuses from the settings window
  const room: SettingsConnectorCommand = { dismiss_case_law_hint: { matter: uuid(1, 2) } };
  expect([ok, room]).toHaveLength(2);
});

test("UC's split: the room's protection command is the set-up alone; Settings has both; no room file imports Settings' Safe workspace", () => {
  const room: RoomProtectionCommand = { retry_protection: {} };
  // @ts-expect-error: removing protection's set-up is Settings' alone
  const notRoom: RoomProtectionCommand = { remove_sandbox_setup: {} };
  const settings: SettingsProtectionCommand[] = [
    { retry_protection: {} },
    { remove_sandbox_setup: {} },
  ];
  expect([room, notRoom, ...settings]).toHaveLength(4);
  const src = join(import.meta.dirname, "..");
  const roomFiles = readdirSync(join(src, "room"), { recursive: true })
    .map((f) => join(src, "room", String(f)))
    .filter((f) => /\.(ts|tsx)$/.test(f));
  for (const file of [...roomFiles, join(src, "main.tsx"), join(src, "App.tsx")])
    expect(readFileSync(file, "utf8"), file).not.toMatch(/settings\/sections\/workspace/);
});
