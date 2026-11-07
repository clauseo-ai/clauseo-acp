// @vitest-environment happy-dom
// Settings, Privacy, "Allowed sites" (0053) over the real settings window and FakeTransport: each
// allowed site with its reach (a matter by its name from the app feed, or always) and its date;
// Remove sending revoke_allowed_site with the site's host, scope and matter, then reading the list
// again; the empty list's line; a refusal's words; a read again when the window regains focus; and
// the room's allow_blocked_site, never sent from here, by type and by source.
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { settled } from "@clauseo/components/lib/late.ts";
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { connectDisplay } from "../../display/connect.ts";
import type { DisplayStore } from "../../room/display-store.ts";
import type { MatterInfo, Settings } from "../../room/host-next.ts";
import { stubFonts } from "../../room/thread/testing.ts";
import { MATTER, uuid } from "../../testing/stream-script.ts";
import { fakeBlocklist, type FakeBlocklistRefusals } from "../../transport/fake-blocklist.ts";
import { FakeTransport } from "../../transport/fake-transport.ts";
import type { AllowedSite, ErrorCode } from "../../transport/protocol.ts";
import { SettingsApp } from "../settings-app.tsx";
import type { SettingsBlocklistCommand } from "./allowed-sites-commands.ts";

vi.mock("../../transport/window-title.ts", () => ({ setWindowTitle: () => {} }));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
stubFonts();

const items = new Map<string, string>();
vi.stubGlobal("localStorage", {
  getItem: (key: string) => items.get(key) ?? null,
  setItem: (key: string, value: string) => void items.set(key, value),
  removeItem: (key: string) => void items.delete(key),
  clear: () => items.clear(),
});

let root: Root | null = null;
let store: DisplayStore | null = null;

beforeEach(() => {
  localStorage.clear();
  history.replaceState(null, "", "/settings.html#privacy");
});

afterEach(async () => {
  await act(async () => root?.unmount());
  document.body.replaceChildren();
  store?.dispose();
  root = null;
  store = null;
});

const SETTINGS: Settings = {
  ui: { theme: "system", font_size: 16, language: "en", lite: "automatic", motion: "system" },
  locked: [],
};
const AT = Date.UTC(2026, 10, 5, 6);
const ALWAYS: AllowedSite = { host: "cdn.example", scope: "always", allowed_at: AT };
const IN_MATTER: AllowedSite = {
  host: "invoice-paypa1.example",
  scope: "matter",
  matter: MATTER,
  allowed_at: AT,
};
const GONE: AllowedSite = {
  host: "old.example",
  scope: "matter",
  matter: uuid(5, 2),
  allowed_at: AT,
};
const NAMED = { matter: MATTER, name: "Sharma v. State of Delhi", kind: "folder" } as MatterInfo;

async function mount({
  sites = [ALWAYS, IN_MATTER],
  refuse = {},
}: {
  sites?: AllowedSite[];
  refuse?: FakeBlocklistRefusals;
} = {}) {
  const blocklist = fakeBlocklist({ sites, refuse });
  const transport = new FakeTransport({
    next: {
      query: (query) => {
        if (query === "get_settings") return { settings: SETTINGS };
        throw new Error("not scripted");
      },
      // the app feed's first frames: the whole matter list
      first: () => [{ matters_changed: { matters: [NAMED], removed: [], all: true } }],
    },
    blocklist,
  });
  const made = connectDisplay(transport).store;
  store = made;
  const el = document.createElement("div");
  document.body.append(el);
  root = createRoot(el);
  await act(async () =>
    root?.render(<SettingsApp store={made} transport={transport} windowControls={null} />),
  );
  await act(async () => settled());
  return { el, transport, blocklist };
}

const rows = (el: HTMLElement) =>
  [...el.querySelectorAll<HTMLElement>('[data-testid="allowed-site"]')].map((r) => r.textContent);
const removeOf = (el: HTMLElement, host: string) =>
  el.querySelector<HTMLElement>(`button[aria-label="Remove \u2068${host}\u2069"]`);
/** The unsafe-site list's commands sent (the window sends others too, such as the connectors'
 * check). */
const blocklistSent = (transport: FakeTransport) =>
  transport
    .only("dispatch")
    .map((c) => c.command.command)
    .filter((c) => c.revoke_allowed_site !== undefined || c.allow_blocked_site !== undefined);
const reads = (transport: FakeTransport) =>
  transport.only("query").filter((c) => c.query === "allowed_sites").length;
const click = async (target: HTMLElement | null) => {
  await act(async () => target?.click());
  await act(async () => settled());
};

test("Privacy lists each allowed site: the host, its reach and when it was allowed", async () => {
  const { el } = await mount({ sites: [ALWAYS, IN_MATTER, GONE] });
  const group = el.querySelector('[data-testid="allowed-sites"]');
  expect(group?.querySelector("h3")?.textContent).toBe("Allowed sites");
  expect(rows(el)).toEqual([
    "cdn.exampleAlways · Allowed 5 November 2026",
    "invoice-paypa1.exampleThis matter: \u2068Sharma v. State of Delhi\u2069 · Allowed 5 November 2026",
    // a matter this window doesn't know (a forgotten matter's allows are kept)
    "old.exampleOne matter · Allowed 5 November 2026",
  ]);
  expect(removeOf(el, "cdn.example")?.textContent).toBe("Remove");
});

test("Remove sends revoke_allowed_site with the matter for a matter allow, and reads the list again", async () => {
  const { el, transport } = await mount();
  const before = reads(transport);
  await click(removeOf(el, "invoice-paypa1.example"));
  expect(blocklistSent(transport)).toEqual([
    { revoke_allowed_site: { host: "invoice-paypa1.example", scope: "matter", matter: MATTER } },
  ]);
  expect(reads(transport)).toBe(before + 1);
  expect(rows(el)).toEqual(["cdn.exampleAlways · Allowed 5 November 2026"]);
});

test("Remove of an always allow names no matter; the last one gone leaves the empty line", async () => {
  const { el, transport } = await mount({ sites: [ALWAYS] });
  await click(removeOf(el, "cdn.example"));
  expect(blocklistSent(transport)).toEqual([
    { revoke_allowed_site: { host: "cdn.example", scope: "always" } },
  ]);
  expect(rows(el)).toEqual([]);
  expect(el.querySelector('[data-testid="allowed-sites-empty"]')?.textContent).toBe(
    "No sites are allowed past the unsafe-site list.",
  );
});

test("an empty list says so", async () => {
  const { el } = await mount({ sites: [] });
  expect(el.querySelector('[data-testid="allowed-sites-empty"]')?.textContent).toBe(
    "No sites are allowed past the unsafe-site list.",
  );
});

test.each([
  ["revoke_allowed_site", "store.io"],
  ["allowed_sites", "command.invalid"],
] as const)("%s refused with %s says its code's words", async (call, code: ErrorCode) => {
  const { el } = await mount({ refuse: { [call]: code } });
  if (call === "revoke_allowed_site") await click(removeOf(el, "cdn.example"));
  expect(el.querySelector('[data-testid="allowed-sites-failed"]')?.textContent).toBe(
    errorWords("en", code),
  );
  // a refused revoke keeps the row
  if (call === "revoke_allowed_site") expect(rows(el)).toHaveLength(2);
});

test("the list is read again when the window regains focus", async () => {
  const { el, transport, blocklist } = await mount({ sites: [] });
  const before = reads(transport);
  blocklist.list().push(ALWAYS);
  await act(async () => window.dispatchEvent(new Event("focus")));
  await act(async () => settled());
  expect(reads(transport)).toBe(before + 1);
  expect(rows(el)).toEqual(["cdn.exampleAlways · Allowed 5 November 2026"]);
});

test("the settings window never sends allow_blocked_site", async () => {
  const { el, transport } = await mount({ sites: [ALWAYS, IN_MATTER] });
  await click(removeOf(el, "cdn.example"));
  await click(removeOf(el, "invoice-paypa1.example"));
  const kinds = blocklistSent(transport).map((c) => Object.keys(c)[0]);
  expect(kinds).toEqual(["revoke_allowed_site", "revoke_allowed_site"]);
  const all = transport.only("dispatch").map((c) => c.command.command);
  expect(all.filter((c) => c.allow_blocked_site !== undefined)).toEqual([]);
  // by type: Settings' vocabulary has no allow
  const ok: SettingsBlocklistCommand = {
    revoke_allowed_site: { host: "a.example", scope: "always" },
  };
  const allow = { allow_blocked_site: { block_id: uuid(9, 7), scope: "always" as const } };
  // @ts-expect-error: the room's command, which the host refuses from the settings window
  const room: SettingsBlocklistCommand = allow;
  expect([ok, room]).toHaveLength(2);
  // by source: nothing under settings/ imports the room's allow, and only it spells the command
  const src = join(import.meta.dirname, "..", "..");
  const files = (dir: string) =>
    readdirSync(join(src, dir), { recursive: true })
      .map((f) => join(src, dir, String(f)))
      .filter((f) => /\.(ts|tsx)$/.test(f) && !/\.test\.tsx?$/.test(f));
  for (const file of files("settings"))
    expect(readFileSync(file, "utf8"), file).not.toMatch(
      /blocklist\/allow\.ts"|allow_blocked_site: \{/,
    );
  const senders = [...files("room"), ...files("settings"), ...files("store")].filter((f) =>
    /allow_blocked_site: \{/.test(readFileSync(f, "utf8")),
  );
  expect(senders.map((f) => f.slice(src.length + 1))).toEqual(["room/blocklist/allow.ts"]);
});
