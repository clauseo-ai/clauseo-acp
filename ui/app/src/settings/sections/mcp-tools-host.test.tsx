// @vitest-environment happy-dom
// Settings › Tools over the real seam (tools-model.ts) on FakeTransport's app feed and dispatch: the view from
// mcp_tools_changed, the empty list before any (the catch-up leaves it out) and after a full one, the commands as
// HostCommand { mcp } and their refusals by code.
import { afterEach, expect, test, vi } from "vitest";
import { act } from "react";
import { stubFonts } from "../../room/thread/testing.ts";
import type { McpToolsView } from "../../transport/protocol.ts";
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
