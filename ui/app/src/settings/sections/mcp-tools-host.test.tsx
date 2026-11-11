// @vitest-environment happy-dom
// Settings › Tools over the real seam (tools-model.ts) on FakeTransport's app feed and dispatch: the view from
// mcp_tools_changed, the empty list before any (the catch-up leaves it out) and after a full one, the commands as
// HostCommand { mcp } and their refusals by code, and import: the buttons from `importable`, the command, the result
// from `last_import` by command id, the host's confirm's Cancel, mcp.import_too_large, and the button while in flight.
import { afterEach, expect, test, vi } from "vitest";
import { act } from "react";
import { stubFonts } from "../../room/thread/testing.ts";
import type { CommandEnvelope, McpImportResult, McpToolsView } from "../../transport/protocol.ts";
import { mount, THREE, type Rig } from "./mcp-tools-rig.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
stubFonts();
vi.mock("../../transport/window-title.ts", () => ({ setWindowTitle: () => {} }));

let rig: Rig | null = null;

afterEach(async () => {
  await rig?.unmount();
  rig = null;
});

test("no event yet: the host leaves an empty list out of its catch-up, so Tools starts empty, then follows each view", async () => {
  rig = await mount(null);
  expect(rig.nav()).toContain("Tools");
  expect(rig.q('[data-testid="tools-section"]')?.getAttribute("data-control")).toBe("lawyer");
  expect(rig.q('[data-testid="tools-empty"]')?.textContent).toBe("No tools yet.");
  expect(rig.q('[data-testid="tools-add"]')).not.toBeNull();
  const one: McpToolsView = {
    control: "lawyer",
    importable: [],
    servers: [
      {
        name: "notes",
        kind: "local",
        command: "uvx",
        args: [],
        env: [],
        headers: [],
        enabled: true,
      },
    ],
  };
  await act(async () => rig!.transport.appEvent({ mcp_tools_changed: one }));
  expect(rig.rows()).toEqual(["notesuvx"]);
  // and back to empty, which the host does send as a change
  await act(async () =>
    rig!.transport.appEvent({
      mcp_tools_changed: { control: "lawyer", servers: [], importable: [] },
    }),
  );
  expect(rig.q('[data-testid="tools-empty"]')).not.toBeNull();
});

test("over the host: the view from mcp_tools_changed, the commands as HostCommand { mcp }, refusals by code, no Import", async () => {
  const sent: unknown[] = [];
  let refuse: string | null = null;
  const view: McpToolsView = {
    control: "lawyer",
    importable: [],
    servers: [
      {
        name: "notes",
        kind: "local",
        command: "uvx",
        args: ["notes-mcp"],
        env: [{ name: "NOTES_TOKEN", saved: true }],
        headers: [],
        enabled: true,
      },
    ],
  };
  rig = await mount(null, {
    first: [{ mcp_tools_changed: view }],
    dispatch: (envelope) => {
      if ("mcp" in envelope.command) sent.push(envelope.command);
      if (refuse !== null) {
        const code = refuse;
        refuse = null;
        throw { code, message: "refused", retryable: false };
      }
      return "done";
    },
  });
  expect(rig.nav()).toContain("Tools");
  expect(rig.rows()).toEqual(["notesnotes-mcp"]);
  expect(rig.q('[data-testid="tools-imports"]')).toBeNull();
  await rig.click(rig.pane().querySelector<HTMLElement>('[role="switch"]'));
  expect(sent.at(-1)).toEqual({ mcp: { set_server_enabled: { name: "notes", enabled: false } } });
  // the list follows the host's next view, not the press
  await act(async () =>
    rig!.transport.appEvent({
      mcp_tools_changed: { ...view, servers: [{ ...view.servers[0]!, enabled: false }] },
    }),
  );
  expect(rig.pane().querySelector('[role="switch"]')?.getAttribute("aria-checked")).toBe("false");
  await rig.press("Edit");
  refuse = "mcp.command_not_found";
  await rig.save();
  expect(sent.at(-1)).toEqual({
    mcp: {
      save_server: {
        previous_name: "notes",
        server: {
          name: "notes",
          kind: "local",
          command: "uvx",
          args: ["notes-mcp"],
          env: [{ name: "NOTES_TOKEN", value: { keep: {} } }],
          // as the host's last view has it: off
          enabled: false,
        },
      },
    },
  });
  expect(rig.errors()).toEqual(["Clauseo couldn't find uvx on this computer."]);
  refuse = "settings.locked";
  await rig.save();
  expect(rig.errors()).toEqual(["Your firm manages tools on this computer."]);
  await rig.save();
  expect(rig.q('[data-testid="tools-form"]')).toBeNull();
  await rig.click(rig.pane().querySelector<HTMLElement>('button[aria-label="Remove notes"]'));
  await rig.click(document.querySelector('[data-testid="tools-remove-ok"]'));
  expect(sent.at(-1)).toEqual({ mcp: { remove_server: { name: "notes" } } });
});

test("an empty view after a full one: every removed tool goes, and the empty state shows", async () => {
  rig = await mount(null, {
    first: [{ mcp_tools_changed: { control: "lawyer", servers: THREE.servers, importable: [] } }],
  });
  expect(rig.rows().length).toBe(3);
  await act(async () =>
    rig!.transport.appEvent({
      mcp_tools_changed: { control: "lawyer", servers: [], importable: [] },
    }),
  );
  expect(rig.rows()).toEqual([]);
  expect(rig.q('[data-testid="tools-empty"]')?.textContent).toBe("No tools yet.");
});

const LISTED: McpToolsView = {
  control: "lawyer",
  servers: [THREE.servers[0]!],
  importable: [
    { source: "claude_desktop", count: 3 },
    { source: "codex", count: 1 },
  ],
};

/** The host's import: each `import` command recorded; `answer` says what it does with it. */
function importHost(answer: (envelope: CommandEnvelope) => Promise<"done">) {
  const sent: CommandEnvelope[] = [];
  return {
    sent,
    dispatch: (envelope: CommandEnvelope) => {
      if (!("mcp" in envelope.command)) return "done" as const;
      sent.push(envelope);
      return answer(envelope);
    },
  };
}

const imports = () =>
  [...document.querySelectorAll('[data-testid="tools-imports"] button')].map((b) => b.textContent);

test("import over the host: buttons from importable, one import command, the lines from last_import by its id, the list from the view", async () => {
  let transport: Rig["transport"] | null = null;
  const host = importHost(async (envelope) => {
    const result: McpImportResult = {
      command: envelope.command_id,
      source: "claude_desktop",
      imported: ["legal-citations"],
      skipped: [
        { name: "filesystem", reason: "command_not_found", program: "npx" },
        { name: "github", reason: "unsupported_transport" },
      ],
      others: 2,
    };
    // another window's import first: not this one's, so not shown
    transport!.appEvent({
      mcp_tools_changed: {
        ...LISTED,
        last_import: { ...result, command: "other", imported: ["x"] },
      },
    });
    transport!.appEvent({
      mcp_tools_changed: {
        control: "lawyer",
        servers: [...LISTED.servers, { ...THREE.servers[1]!, name: "legal-citations" }],
        importable: [{ source: "codex", count: 1 }],
        last_import: result,
      },
    });
    return "done";
  });
  rig = await mount(null, { first: [{ mcp_tools_changed: LISTED }], dispatch: host.dispatch });
  transport = rig.transport;
  expect(imports()).toEqual(["Import 3 tools from Claude Desktop", "Import 1 tool from Codex"]);
  await rig.press("Import 3 tools from Claude Desktop");
  expect(host.sent.map((e) => e.command)).toEqual([
    { mcp: { import: { source: "claude_desktop" } } },
  ]);
  expect(
    [...rig.pane().querySelectorAll('[data-testid="tools-imported"] p')].map((p) => p.textContent),
  ).toEqual([
    "Imported 1 tool. 4 couldn't be imported.",
    "filesystem: Clauseo couldn't find npx on this computer.",
    "github: it uses a setting Clauseo doesn't support yet.",
    "2 more couldn't be imported.",
  ]);
  expect(rig.rows().map((r) => r.startsWith("legal-citations"))).toContain(true);
  expect(imports()).toEqual(["Import 1 tool from Codex"]);
});

test("the last_import may come before Done: the import still shows it", async () => {
  let transport: Rig["transport"] | null = null;
  const host = importHost(async (envelope) => {
    transport!.appEvent({
      mcp_tools_changed: {
        ...LISTED,
        last_import: {
          command: envelope.command_id,
          source: "codex",
          imported: ["docket"],
          skipped: [],
        },
      },
    });
    return "done";
  });
  rig = await mount(null, { first: [{ mcp_tools_changed: LISTED }], dispatch: host.dispatch });
  transport = rig.transport;
  await rig.press("Import 1 tool from Codex");
  expect(rig.q('[data-testid="tools-imported-head"]')?.textContent).toBe("Imported 1 tool.");
});

test("no buttons when importable is empty, nor under the firm's list", async () => {
  rig = await mount(null, { first: [{ mcp_tools_changed: { ...LISTED, importable: [] } }] });
  expect(rig.q('[data-testid="tools-imports"]')).toBeNull();
  await act(async () =>
    rig!.transport.appEvent({ mcp_tools_changed: { ...LISTED, control: "firm_list" } }),
  );
  expect(rig.q('[data-testid="tools-imports"]')).toBeNull();
});

test("Cancel in the host's confirm (mcp.save_declined): the section as it was, no line", async () => {
  const host = importHost(async () => {
    throw { code: "mcp.save_declined", message: "declined", retryable: false };
  });
  rig = await mount(null, { first: [{ mcp_tools_changed: LISTED }], dispatch: host.dispatch });
  const before = rig.text();
  await rig.press("Import 1 tool from Codex");
  expect(host.sent.length).toBe(1);
  expect(rig.q('[data-testid="tools-import-failed"]')).toBeNull();
  expect(rig.q('[data-testid="tools-failed"]')).toBeNull();
  expect(rig.q('[data-testid="tools-imported"]')).toBeNull();
  expect(rig.text()).toBe(before);
});

test("mcp.import_too_large: its line, in the section's refusal style; nothing else changes", async () => {
  const host = importHost(async () => {
    throw { code: "mcp.import_too_large", message: "too large", retryable: false };
  });
  rig = await mount(null, { first: [{ mcp_tools_changed: LISTED }], dispatch: host.dispatch });
  await rig.press("Import 3 tools from Claude Desktop");
  const line = rig.q('[data-testid="tools-import-failed"]');
  expect(line?.textContent).toBe(
    "There are too many tools to import at once, so nothing was imported. Add them one at a time.",
  );
  expect(line?.getAttribute("role")).toBe("status");
  expect(line?.className).toBe("set-note");
  expect(rig.rows().length).toBe(1);
});

test("while an import is in flight: its button says Importing… and every Import button is off; then back", async () => {
  let go: (() => void) | null = null;
  let transport: Rig["transport"] | null = null;
  const host = importHost(async (envelope) => {
    await new Promise<void>((resolve) => (go = resolve));
    transport!.appEvent({
      mcp_tools_changed: {
        ...LISTED,
        last_import: { command: envelope.command_id, source: "codex", imported: [], skipped: [] },
      },
    });
    return "done";
  });
  rig = await mount(null, { first: [{ mcp_tools_changed: LISTED }], dispatch: host.dispatch });
  transport = rig.transport;
  await rig.press("Import 1 tool from Codex");
  const buttons = () => [
    ...document.querySelectorAll<HTMLButtonElement>('[data-testid="tools-imports"] button'),
  ];
  expect(buttons().map((b) => b.textContent)).toEqual([
    "Import 3 tools from Claude Desktop",
    "Importing…",
  ]);
  expect(buttons().every((b) => b.disabled)).toBe(true);
  expect(buttons()[1]!.getAttribute("aria-busy")).toBe("true");
  // a second press does nothing
  await rig.click(buttons()[0]!);
  expect(host.sent.length).toBe(1);
  await act(async () => go!());
  for (let i = 0; i < 3; i += 1) await act(async () => {});
  expect(buttons().map((b) => b.disabled)).toEqual([false, false]);
  expect(imports()).toEqual(["Import 3 tools from Claude Desktop", "Import 1 tool from Codex"]);
});
