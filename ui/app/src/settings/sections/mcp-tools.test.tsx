// @vitest-environment happy-dom
// Settings › Tools (UC-169 to UC-189) over the fake host (tools-fake.ts): the list, its switch, adding, editing with
// saved environment values (kept, replaced, removed), each refusal's place and words, the firm's two states, Remove's
// confirm, Escape, and REMOTE_TOOLS hiding the address form. No real secret: the fake hides every value it is sent.
import { afterEach, expect, test, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { settled } from "@clauseo/components/lib/late.ts";
import { connectDisplay } from "../../display/connect.ts";
import type { Settings } from "../../room/host-next.ts";
import { stubFonts } from "../../room/thread/testing.ts";
import { FakeTransport } from "../../transport/fake-transport.ts";
import { SettingsApp } from "../settings-app.tsx";
import { commandExample, refusalOf } from "./mcp-tools-refusal.ts";
import { FakeTools, local } from "./tools-fake.ts";
import {
  fileName,
  hostOf,
  isFullPath,
  provideTools,
  REMOTE_TOOLS,
  type McpToolsError,
  type McpToolsView,
} from "./tools-model.ts";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
stubFonts();
vi.mock("../../transport/window-title.ts", () => ({ setWindowTitle: () => {} }));

const SETTINGS: Settings = {
  ui: { theme: "system", font_size: 16, language: "en", lite: "automatic", motion: "system" },
  locked: [],
};

const PLAIN =
  "A tool runs on this computer. It can reach anything your account here can: your files, your programs and the internet.";

const THREE: McpToolsView = {
  control: "lawyer",
  servers: [
    local("court-diary", "/opt/homebrew/bin/npx", { args: ["-y", "court-diary-mcp"] }),
    local("filings", "/Users/r/tools/filings-server", { enabled: false }),
    local("notes", "/usr/local/bin/uvx", { env: [{ name: "NOTES_TOKEN", saved: true }] }),
  ],
};

type Rig = Awaited<ReturnType<typeof mount>>;
let rig: Rig | null = null;

afterEach(async () => {
  await rig?.unmount();
  rig = null;
});

async function mount(tools: FakeTools | null) {
  history.replaceState(null, "", "/settings.html#tools");
  const transport = new FakeTransport({
    next: {
      query: (query) => {
        if (query === "get_settings") return { settings: SETTINGS };
        throw new Error("not scripted");
      },
    },
  });
  if (tools !== null) provideTools(transport, tools);
  const store = connectDisplay(transport).store;
  const el = document.createElement("div");
  document.body.append(el);
  const root: Root = createRoot(el);
  const settle = async () => {
    for (let i = 0; i < 3; i += 1) await act(async () => settled());
  };
  await act(async () =>
    root.render(<SettingsApp store={store} transport={transport} windowControls={null} />),
  );
  await settle();
  const pane = () => el.querySelector("main.set-pane") as HTMLElement;
  const q = <E extends Element = HTMLElement>(sel: string) => pane().querySelector<E>(sel);
  const button = (words: string) =>
    [...document.querySelectorAll<HTMLButtonElement>("button")].find(
      (b) => b.textContent === words,
    );
  const type = async (target: HTMLInputElement | HTMLTextAreaElement, text: string) => {
    const proto =
      target instanceof HTMLTextAreaElement
        ? HTMLTextAreaElement.prototype
        : HTMLInputElement.prototype;
    await act(async () => {
      Reflect.set(proto, "value", text, target);
      target.dispatchEvent(new Event("input", { bubbles: true }));
    });
  };
  return {
    el,
    transport,
    pane,
    q,
    text: () => pane()?.textContent ?? "",
    nav: () =>
      [...el.querySelectorAll(".set-nav-row .set-nav-name")].map((n) => n.textContent ?? ""),
    rows: () =>
      [...pane().querySelectorAll('[data-testid="tools-row"]')].map((r) => r.textContent ?? ""),
    errors: () =>
      [...pane().querySelectorAll('[data-testid="tools-error"]')].map((r) => r.textContent ?? ""),
    press: async (words: string) => {
      const b = button(words);
      if (b === undefined) throw new Error(`no button "${words}"`);
      await act(async () => b.click());
      await settle();
    },
    click: async (target: HTMLElement | null) => {
      if (target === null) throw new Error("nothing to click");
      await act(async () => target.click());
      await settle();
    },
    has: (words: string) => button(words) !== undefined,
    type,
    field: (id: string) => q<HTMLInputElement>(`[data-testid="${id}"]`)!,
    save: async () => {
      await act(async () => q<HTMLFormElement>('[data-testid="tools-form"]')!.requestSubmit());
      await settle();
    },
    unmount: async () => {
      await act(async () => root.unmount());
      el.remove();
      store.dispose();
    },
  };
}

test("without the host's commands the section doesn't show", async () => {
  rig = await mount(null);
  expect(rig.nav()).not.toContain("Tools");
});

test("the list: each tool's name and its program's file name, the plain line, the Safe workspace's line", async () => {
  rig = await mount(new FakeTools(THREE));
  expect(rig.nav()).toContain("Tools");
  expect(rig.el.querySelector("h2")?.textContent).toBe("Tools");
  expect(rig.text()).toContain(PLAIN);
  expect(rig.text()).toContain("Not used in the Safe workspace.");
  expect(rig.rows()).toEqual(["court-diarynpx", "filingsfilings-server", "notesuvx"]);
  const switches = [...rig.pane().querySelectorAll('[role="switch"]')];
  expect(switches.map((s) => s.getAttribute("aria-checked"))).toEqual(["true", "false", "true"]);
  expect(switches.map((s) => s.getAttribute("aria-label"))).toEqual([
    "court-diary",
    "filings",
    "notes",
  ]);
  expect(rig.has("Edit")).toBe(true);
  expect(rig.has("Add a tool")).toBe(true);
});

test("empty: one line and Add a tool", async () => {
  rig = await mount(new FakeTools({ control: "lawyer", servers: [] }));
  expect(rig.q('[data-testid="tools-empty"]')?.textContent).toBe("No tools yet.");
  expect(rig.has("Add a tool")).toBe(true);
});

test("the switch turns a tool off and on, one command each", async () => {
  const fake = new FakeTools(THREE);
  rig = await mount(fake);
  const first = () => rig!.pane().querySelectorAll<HTMLElement>('[role="switch"]')[0]!;
  await rig.click(first());
  expect(fake.toggled).toEqual([{ name: "court-diary", enabled: false }]);
  expect(first().getAttribute("aria-checked")).toBe("false");
  await rig.click(first());
  expect(fake.toggled.at(-1)).toEqual({ name: "court-diary", enabled: true });
});

test("add: name, full path, arguments one per line, a new value; the list shows it and focus goes to its Edit", async () => {
  const fake = new FakeTools({ control: "lawyer", servers: [] });
  rig = await mount(fake);
  await rig.press("Add a tool");
  expect(document.activeElement).toBe(rig.field("tools-name"));
  await rig.type(rig.field("tools-name"), "court-diary");
  await rig.type(rig.field("tools-command"), "/opt/homebrew/bin/npx");
  await rig.type(
    rig.field("tools-args") as unknown as HTMLTextAreaElement,
    "-y\n court-diary-mcp \n\n",
  );
  await rig.click(rig.q('[data-testid="tools-add-env"]'));
  expect(document.activeElement).toBe(rig.field("tools-secret-name"));
  await rig.type(rig.field("tools-secret-name"), "DIARY_TOKEN");
  await rig.type(rig.field("tools-secret-value"), "fake-value");
  expect(rig.field("tools-secret-value").type).toBe("password");
  await rig.save();
  expect(fake.saves).toEqual([
    {
      previousName: null,
      server: {
        name: "court-diary",
        kind: "local",
        command: "/opt/homebrew/bin/npx",
        args: ["-y", "court-diary-mcp"],
        url: null,
        env: [{ name: "DIARY_TOKEN", value: { set: "(set)" } }],
        headers: [],
        enabled: true,
      },
    },
  ]);
  expect(fake.holds("court-diary", "env", "DIARY_TOKEN")).toBe(true);
  expect(rig.q('[data-testid="tools-form"]')).toBeNull();
  expect(rig.rows()).toEqual(["court-diarynpx"]);
  expect(document.activeElement?.getAttribute("data-tool-edit")).toBe("court-diary");
});

test("edit with saved values: one kept, one replaced, one removed; no saved value is ever in the page", async () => {
  const fake = new FakeTools({
    control: "lawyer",
    servers: [
      local("notes", "/usr/local/bin/uvx", {
        args: ["notes-mcp"],
        env: [
          { name: "KEEP_ME", saved: true },
          { name: "REPLACE_ME", saved: true },
          { name: "REMOVE_ME", saved: true },
        ],
      }),
    ],
  });
  for (const name of ["KEEP_ME", "REPLACE_ME", "REMOVE_ME"]) fake.seed("notes", "env", name);
  rig = await mount(fake);
  await rig.press("Edit");
  expect(rig.field("tools-name").value).toBe("notes");
  expect(rig.field("tools-command").value).toBe("/usr/local/bin/uvx");
  expect(rig.pane().querySelectorAll('[data-testid="tools-saved"]').length).toBe(3);
  // saved values show no field at all until Replace
  expect(rig.pane().querySelectorAll('[data-testid="tools-secret-value"]').length).toBe(0);
  expect(rig.text()).not.toContain("seeded");
  const secretRow = (name: string) =>
    [...rig!.pane().querySelectorAll<HTMLElement>('[data-testid="tools-secret"]')].find((r) =>
      r.textContent?.startsWith(name),
    )!;
  const inRow = (name: string, words: string) =>
    [...secretRow(name).querySelectorAll<HTMLButtonElement>("button")].find(
      (b) => b.textContent === words,
    )!;
  await rig.click(inRow("REPLACE_ME", "Replace"));
  const value = secretRow("REPLACE_ME").querySelector<HTMLInputElement>("input")!;
  expect(value.type).toBe("password");
  expect(document.activeElement).toBe(value);
  await rig.type(value, "fake-new-value");
  await rig.click(inRow("REMOVE_ME", "Remove"));
  await rig.save();
  expect(fake.saves.at(-1)?.server.env).toEqual([
    { name: "KEEP_ME", value: { keep: true } },
    { name: "REPLACE_ME", value: { set: "(set)" } },
  ]);
  expect(fake.saves.at(-1)?.previousName).toBe("notes");
  expect(fake.holds("notes", "env", "KEEP_ME")).toBe(true);
  expect(fake.holds("notes", "env", "REPLACE_ME")).toBe(true);
  expect(fake.holds("notes", "env", "REMOVE_ME")).toBe(false);
});

test("Replace left empty keeps the saved value", async () => {
  const fake = new FakeTools({
    control: "lawyer",
    servers: [local("notes", "/usr/local/bin/uvx", { env: [{ name: "TOKEN", saved: true }] })],
  });
  fake.seed("notes", "env", "TOKEN");
  rig = await mount(fake);
  await rig.press("Edit");
  await rig.press("Replace");
  await rig.save();
  expect(fake.saves.at(-1)?.server.env).toEqual([{ name: "TOKEN", value: { keep: true } }]);
});

test("a bare program name: the form says to use the full path, with an example, and sends nothing", async () => {
  const fake = new FakeTools({ control: "lawyer", servers: [] });
  rig = await mount(fake);
  await rig.press("Add a tool");
  await rig.type(rig.field("tools-name"), "diary");
  await rig.type(rig.field("tools-command"), "npx");
  await rig.save();
  expect(fake.saves).toEqual([]);
  // the example is this computer's system's (the test's display is Linux's)
  expect(rig.errors()).toEqual([
    `Use the program's full path, such as ${commandExample(document.documentElement.dataset["platform"] ?? "")}.`,
  ]);
  expect(rig.errors()[0]).toMatch(/^Use the program's full path, such as \/.+\/npx\.$/);
  expect(rig.field("tools-command").getAttribute("aria-invalid")).toBe("true");
});

test("each refusal the host sends shows in its place, in its words", async () => {
  const cases: [McpToolsError, string][] = [
    ["nameEmpty", "Give the tool a name."],
    ["nameTaken", "That name can't be used. Choose another."],
    ["nameReserved", "That name can't be used. Choose another."],
    ["nameInvalid", "Use letters, numbers, - and _ only."],
    ["commandNotAbsolute", "Use the program's full path, such as /opt/homebrew/bin/npx."],
    ["urlInvalid", "Use a full address, such as https://example.com/mcp."],
    ["envNameInvalid", "Use letters, numbers, - and _ only."],
    ["headerNameInvalid", "Use letters, numbers, - and _ only."],
    ["lockedByFirm", "Your firm manages tools on this computer."],
    ["secretStoreFailed", "Clauseo couldn't save that setting. Your settings are as they were."],
  ];
  const places: Record<McpToolsError, string> = {
    nameEmpty: "name",
    nameTaken: "name",
    nameReserved: "name",
    nameInvalid: "name",
    commandNotAbsolute: "command",
    urlInvalid: "url",
    envNameInvalid: "env",
    headerNameInvalid: "headers",
    lockedByFirm: "form",
    secretStoreFailed: "form",
  };
  for (const [code, words] of cases)
    expect(refusalOf("en", "mac", code)).toEqual({ place: places[code], words });
  expect(refusalOf("en", "windows", "commandNotAbsolute").words).toBe(
    "Use the program's full path, such as C:\\Program Files\\nodejs\\npx.cmd.",
  );
  // in the form: each refusal the host answers a well-formed save with, under its field
  const fake = new FakeTools({ control: "lawyer", servers: [] });
  rig = await mount(fake);
  await rig.press("Add a tool");
  await rig.type(rig.field("tools-name"), "diary");
  await rig.type(rig.field("tools-command"), "/opt/homebrew/bin/npx");
  for (const code of [
    "nameTaken",
    "envNameInvalid",
    "secretStoreFailed",
    "lockedByFirm",
  ] as const) {
    fake.refuseNext = code;
    await rig.save();
    expect(rig.errors()).toEqual([refusalOf("en", "mac", code).words]);
    expect(rig.q('[data-testid="tools-form"]')).not.toBeNull();
  }
  // the name field carries its refusal
  fake.refuseNext = "nameReserved";
  await rig.save();
  expect(rig.field("tools-name").getAttribute("aria-invalid")).toBe("true");
});

test("Remove asks first; Cancel keeps the tool, Remove sends one remove", async () => {
  const fake = new FakeTools(THREE);
  rig = await mount(fake);
  const removeOf = (name: string) =>
    rig!.pane().querySelector<HTMLElement>(`button[aria-label="Remove ${name}"]`);
  await rig.click(removeOf("filings"));
  expect(document.querySelector('[data-testid="tools-remove-confirm"]')?.textContent).toContain(
    "Remove filings?",
  );
  await rig.click(document.querySelector('[data-testid="tools-remove-cancel"]'));
  expect(fake.removed).toEqual([]);
  await rig.click(removeOf("filings"));
  await rig.click(document.querySelector('[data-testid="tools-remove-ok"]'));
  expect(fake.removed).toEqual(["filings"]);
  expect(rig.rows()).toEqual(["court-diarynpx", "notesuvx"]);
});

test("Escape and Cancel leave the form without saving, focus back where it was", async () => {
  const fake = new FakeTools(THREE);
  rig = await mount(fake);
  await rig.press("Add a tool");
  await rig.type(rig.field("tools-name"), "half-done");
  await act(async () =>
    rig!
      .field("tools-name")
      .dispatchEvent(
        new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }),
      ),
  );
  await act(async () => settled());
  expect(rig.q('[data-testid="tools-form"]')).toBeNull();
  expect(document.activeElement?.getAttribute("data-testid")).toBe("tools-add");
  await rig.click(rig.pane().querySelector('button[aria-label="Edit notes"]'));
  await rig.press("Cancel");
  expect(document.activeElement?.getAttribute("data-tool-edit")).toBe("notes");
  expect(fake.saves).toEqual([]);
});

test("firmList: the list read-only with the lock's treatment, and the firm's line", async () => {
  rig = await mount(new FakeTools({ ...THREE, control: "firmList" }));
  expect(rig.text()).toContain("Your firm set these tools.");
  expect(rig.rows().length).toBe(3);
  expect(rig.has("Edit")).toBe(false);
  expect(rig.has("Remove")).toBe(false);
  expect(rig.has("Add a tool")).toBe(false);
  for (const s of rig.pane().querySelectorAll<HTMLButtonElement>('[role="switch"]'))
    expect(
      s.disabled || s.getAttribute("aria-disabled") === "true" || s.hasAttribute("data-disabled"),
    ).toBe(true);
  expect(rig.pane().querySelectorAll(".set-tools-row[data-disabled]").length).toBe(3);
});

test("firmManaged: one line and no list", async () => {
  rig = await mount(new FakeTools({ ...THREE, control: "firmManaged" }));
  expect(rig.q('[data-testid="tools-section"]')?.textContent).toBe(
    "Your firm manages tools on this computer.",
  );
  expect(rig.rows()).toEqual([]);
  expect(rig.has("Add a tool")).toBe(false);
  expect(rig.text()).not.toContain("Not used in the Safe workspace.");
});

test("REMOTE_TOOLS off: adding offers no address; a preview turns the address form on", async () => {
  expect(REMOTE_TOOLS).toBe(false);
  rig = await mount(new FakeTools({ control: "lawyer", servers: [] }));
  await rig.press("Add a tool");
  expect(rig.has("Address")).toBe(false);
  expect(rig.q('[data-testid="tools-url"]')).toBeNull();
  await rig.unmount();
  const fake = new FakeTools({ control: "lawyer", servers: [] }, { previewRemote: true });
  rig = await mount(fake);
  await rig.press("Add a tool");
  await rig.press("Address");
  expect(rig.q('[data-testid="tools-form"]')?.getAttribute("data-kind")).toBe("remote");
  await rig.type(rig.field("tools-name"), "research");
  await rig.type(rig.field("tools-url"), "https://mcp.example.com/research");
  await rig.click(rig.q('[data-testid="tools-add-header"]'));
  await rig.type(rig.field("tools-secret-name"), "Authorization");
  await rig.type(rig.field("tools-secret-value"), "fake-header");
  await rig.save();
  expect(fake.saves.at(-1)?.server).toMatchObject({
    kind: "remote",
    command: null,
    url: "https://mcp.example.com/research",
    headers: [{ name: "Authorization", value: { set: "(set)" } }],
    env: [],
  });
  expect(rig.rows()).toEqual(["researchmcp.example.com"]);
});

test("the model's helpers: full paths, file names, hosts, the example per system", () => {
  expect(isFullPath("/opt/homebrew/bin/npx")).toBe(true);
  expect(isFullPath("C:\\Program Files\\nodejs\\npx.cmd")).toBe(true);
  expect(isFullPath("\\\\server\\tools\\run.exe")).toBe(true);
  expect(isFullPath("npx")).toBe(false);
  expect(isFullPath("./bin/npx")).toBe(false);
  expect(fileName("C:\\Program Files\\nodejs\\npx.cmd")).toBe("npx.cmd");
  expect(fileName("/usr/local/bin/uvx")).toBe("uvx");
  expect(hostOf("https://mcp.example.com:8443/x")).toBe("mcp.example.com:8443");
  expect(hostOf("not an address")).toBe("not an address");
  expect(commandExample("linux")).toBe("/usr/bin/npx");
});
