// @vitest-environment happy-dom
// The window split (U15): Settings sends the connector commands; the room (the main window) sends none of them. Over
// the room as App mounts it, with every ConnectorStatus played, available and not, the fake host's dispatched list
// holds no connector command; no file under room/ or the room's entry imports the settings window's connector code;
// and Settings' command type can't carry the room's dismiss_case_law_hint.
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
import type { SettingsConnectorCommand } from "./connector-commands.ts";

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

test("Settings' command type has no dismiss_case_law_hint", () => {
  const ok: SettingsConnectorCommand = { add: { id: "case-law" } };
  // @ts-expect-error: the room's command, which the host refuses from the settings window
  const room: SettingsConnectorCommand = { dismiss_case_law_hint: { matter: uuid(1, 2) } };
  expect([ok, room]).toHaveLength(2);
});
