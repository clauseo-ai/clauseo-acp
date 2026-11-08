// @vitest-environment happy-dom
// Settings, "Remembered answers" (R18) over the real settings window and FakeTransport
// (fake-allowances.ts): the rows grouped by matter, named from the app feed (a matter it doesn't
// know says so), each the folder or site isolated, its kind and its answer; Forget sending
// forget_allowance and taking the row off; the empty list's line and the line under a list; a
// refusal's words; the list read again on focus, on becoming visible and after a Forget, with no
// matter's stream followed; and nothing here switches the Safe workspace off or on, on screen or in the
// source.
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { settled } from "@clauseo/components/lib/late.ts";
import { aw } from "@clauseo/components/lib/words-allowances.ts";
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { connectDisplay } from "../../display/connect.ts";
import type { DisplayStore } from "../../room/display-store.ts";
import type { MatterInfo, Settings } from "../../room/host-next.ts";
import { stubFonts } from "../../room/thread/testing.ts";
import { MATTER, uuid } from "../../testing/stream-script.ts";
import {
  fakeAllowances,
  withAllowances,
  type FakeAllowancesRefusals,
} from "../../transport/fake-allowances.ts";
import { FakeTransport } from "../../transport/fake-transport.ts";
import type { MatterAllowance } from "../../transport/protocol.ts";
import { SettingsApp } from "../settings-app.tsx";
import type { SettingsAllowanceCommand } from "./allowances-commands.ts";

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
  history.replaceState(null, "", "/settings.html#allowances");
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
const RAO = uuid(6, 2);
const GONE = uuid(9, 2);
const SHARMA = { matter: MATTER, name: "Sharma v. State of Delhi", kind: "folder" } as MatterInfo;
const RAO_ROW = { matter: RAO, name: "Rao v. Union of India", kind: "folder" } as MatterInfo;
const kept = (matter: string, n: number, target: MatterAllowance["target"], allowed = true) =>
  ({ matter, allowance: uuid(n, 8), target, mode: "read_only", allowed, at: 0 }) as MatterAllowance;
const EXHIBITS = kept(MATTER, 1, { folder: { path: "/Users/asha/Desktop/Exhibits" } });
const SHARE = kept(MATTER, 2, { site: { host: "files-share.example" } }, false);
const ORDERS = kept(RAO, 3, { file: { path: "/Users/asha/Orders/2026-03.pdf" } });
const OLD = kept(GONE, 4, { site: { host: "old-portal.example" } });

async function mount({
  allowances = [EXHIBITS, SHARE, ORDERS],
  refuse = {},
}: { allowances?: MatterAllowance[]; refuse?: FakeAllowancesRefusals } = {}) {
  const allow = fakeAllowances({ allowances, refuse });
  const transport = new FakeTransport({
    ...withAllowances(
      {
        query: () => {
          throw new Error("not scripted");
        },
      },
      allow,
    ),
    next: {
      query: (query) => {
        if (query === "get_settings") return { settings: SETTINGS };
        throw new Error("not scripted");
      },
      first: () => [{ matters_changed: { matters: [SHARMA, RAO_ROW], removed: [], all: true } }],
    },
    // the section needs the protection wire, as the Safe workspace does
    protection: {
      view: () => ({
        tier: "a",
        computer: { protected: { tier: "a" } },
        required: false,
        unprotected_auto_allowed: true,
      }),
    },
  });
  allow.bind(transport);
  const made = connectDisplay(transport).store;
  store = made;
  const el = document.createElement("div");
  document.body.append(el);
  root = createRoot(el);
  await act(async () =>
    root?.render(<SettingsApp store={made} transport={transport} windowControls={null} />),
  );
  await settle();
  return { el, transport, allow };
}

const settle = async () => {
  for (let i = 0; i < 4; i += 1) await act(async () => settled());
};
const section = (el: HTMLElement) => el.querySelector<HTMLElement>('[data-testid="allowances"]');
/** Without the isolates (FSI, PDI) around each name and path: what the rows say. The isolation itself has its own
 * test below. */
const bare = (text: string | null | undefined) => text?.replace(/[\u2068\u2069]/g, "");
const groups = (el: HTMLElement) =>
  [...el.querySelectorAll<HTMLElement>('[data-testid="allowances-matter"]')].map((g) => [
    bare(g.querySelector("h3")?.textContent),
    ...[...g.querySelectorAll('[data-testid="remembered-row"]')].map((r) => bare(r.textContent)),
  ]);
const forgetOf = (el: HTMLElement, target: string) =>
  el.querySelector<HTMLElement>(`button[aria-label="Forget \u2068${target}\u2069"]`);
const sent = (transport: FakeTransport) =>
  transport
    .only("dispatch")
    .map((c) => c.command.command)
    .filter(
      (c) =>
        c.forget_allowance !== undefined ||
        c.request_escalation !== undefined ||
        c.switch_protection_on !== undefined,
    );
const reads = (transport: FakeTransport) =>
  transport.only("query").filter((q) => q.query === "list_allowances").length;
const click = async (target: HTMLElement | null) => {
  await act(async () => target?.click());
  await settle();
};

test("the remembered answers, grouped by matter: each folder or site isolated, its kind and its answer", async () => {
  const { el, transport } = await mount({ allowances: [EXHIBITS, SHARE, ORDERS, OLD] });
  expect(transport.only("query").filter((q) => q.query === "list_allowances")).toHaveLength(1);
  expect(groups(el)).toEqual([
    [
      "Sharma v. State of Delhi",
      "/Users/asha/Desktop/ExhibitsFolder, read only · Allowed",
      "files-share.exampleSite · Not allowed",
    ],
    ["Rao v. Union of India", "/Users/asha/Orders/2026-03.pdfFile, read only · Allowed"],
    // a matter this window doesn't know
    [aw("en", "A matter not on your list"), "old-portal.exampleSite · Allowed"],
  ]);
  expect(section(el)?.querySelector("bdi")?.textContent).toBe(
    "\u2068/Users/asha/Desktop/Exhibits\u2069",
  );
  expect(section(el)?.querySelector("h3")?.textContent).toBe(
    "\u2068Sharma v. State of Delhi\u2069",
  );
  expect(el.querySelector('[data-testid="allowances-later"]')?.textContent).toBe(
    aw("en", "Forgetting a folder takes effect the next time Clauseo starts work in that matter."),
  );
  expect(forgetOf(el, "files-share.example")?.textContent).toBe("Forget");
});

test("Forget sends forget_allowance with the matter and the answer, and the row goes", async () => {
  const { el, transport, allow } = await mount();
  await click(forgetOf(el, "/Users/asha/Desktop/Exhibits"));
  expect(reads(transport)).toBe(2);
  expect(sent(transport)).toEqual([
    { forget_allowance: { matter: MATTER, allowance: EXHIBITS.allowance } },
  ] satisfies SettingsAllowanceCommand[]);
  expect(allow.list().map((a) => a.allowance)).toEqual([SHARE.allowance, ORDERS.allowance]);
  expect(groups(el)[0]).toEqual([
    "Sharma v. State of Delhi",
    "files-share.exampleSite · Not allowed",
  ]);
});

test("the last answer forgotten leaves the empty list's line", async () => {
  const { el } = await mount({ allowances: [SHARE] });
  await click(forgetOf(el, "files-share.example"));
  expect(groups(el)).toEqual([]);
  expect(el.querySelector('[data-testid="allowances-empty"]')).not.toBeNull();
});

test("no remembered answers: the empty list's line, and no line under it", async () => {
  const { el } = await mount({ allowances: [] });
  expect(el.querySelector('[data-testid="allowances-empty"]')?.textContent).toBe(
    aw(
      "en",
      "No remembered answers. When you allow or refuse a folder or site for a matter and keep the answer, it shows here.",
    ),
  );
  expect(el.querySelector('[data-testid="allowances-later"]')).toBeNull();
});

test("a refused Forget says its words, and the row stays", async () => {
  const { el } = await mount({ refuse: { forget_allowance: "internal" } });
  await click(forgetOf(el, "files-share.example"));
  expect(el.querySelector('[data-testid="allowances-failed"]')?.textContent).toBe(
    errorWords("en", "internal"),
  );
  expect(groups(el)[0]).toHaveLength(3);
});

test("a refused read says its words", async () => {
  const { el } = await mount({ refuse: { list_allowances: "internal" } });
  expect(el.querySelector('[data-testid="allowances-failed"]')?.textContent).toBe(
    errorWords("en", "internal"),
  );
});

test("a focus re-reads: an answer recorded in the room, one forgotten elsewhere", async () => {
  const { el, transport, allow } = await mount();
  expect(reads(transport)).toBe(1);
  // no matter's stream is followed from this window
  expect(transport.only("subscribe")).toEqual([]);
  allow.recorded({
    matter: RAO,
    target: { site: { host: "ecourts.example" } },
    mode: "read_only",
    allowed: true,
  });
  allow.forgotten(MATTER, EXHIBITS.allowance);
  await act(async () => window.dispatchEvent(new Event("focus")));
  await settle();
  expect(reads(transport)).toBe(2);
  expect(groups(el)).toEqual([
    ["Sharma v. State of Delhi", "files-share.exampleSite · Not allowed"],
    [
      "Rao v. Union of India",
      "/Users/asha/Orders/2026-03.pdfFile, read only · Allowed",
      "ecourts.exampleSite · Allowed",
    ],
  ]);
  // and when the window becomes visible again
  await act(async () => document.dispatchEvent(new Event("visibilitychange")));
  await settle();
  expect(reads(transport)).toBe(3);
});

test("Forget re-reads the list once the host answers, refused or not", async () => {
  const { el, transport } = await mount({ refuse: { forget_allowance: "internal" } });
  await click(forgetOf(el, "files-share.example"));
  expect(reads(transport)).toBe(2);
});

test("the settings window never offers switching the Safe workspace off or on", async () => {
  const { el } = await mount();
  const text = document.body.textContent ?? "";
  expect(section(el)).not.toBeNull();
  for (const words of [
    aw("en", "Switch off the Safe workspace…"),
    aw("en", "Switch the Safe workspace back on"),
    aw("en", "Switch it back on"),
    aw("en", "Use files from another matter…"),
  ])
    expect(text).not.toContain(words);
  // nor can its source send them: no settings file names those commands or the room's module
  const dir = join(import.meta.dirname, "..");
  const files = readdirSync(dir, { recursive: true })
    .map(String)
    .filter((f) => /\.tsx?$/.test(f) && !/\.test\.tsx?$/.test(f));
  for (const file of files) {
    const source = readFileSync(join(dir, file), "utf8");
    expect(source, file).not.toMatch(
      /switch_protection_o(n|ff)|files_from_other_matter|room\/allowances/,
    );
  }
});

test("a SettingsAllowanceCommand can't carry anything but forget_allowance", () => {
  // @ts-expect-error the settings window has no typed way to switch the Safe workspace off
  const off: SettingsAllowanceCommand = { switch_protection_on: { matter: MATTER } };
  void off;
});

test("a path with a right-to-left override and a line break shows as one isolated run, with neither", async () => {
  const odd = kept(MATTER, 9, { folder: { path: "/Users/asha/\u202Efdp.exe\nFake line" } });
  const { el } = await mount({ allowances: [odd] });
  const text = el.querySelector('[data-testid="remembered-row"] bdi')?.textContent ?? "";
  expect(text).toBe("\u2068/Users/asha/fdp.exeFake line\u2069");
  expect(text).not.toMatch(/[\u202E\n]/);
  expect(forgetOf(el, "/Users/asha/fdp.exeFake line")).not.toBeNull();
});
