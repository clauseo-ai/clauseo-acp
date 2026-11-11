// Settings › Tools' scenes (UC-169 to UC-189), for the probe's ?tools= pages and the copy review: each a view the fake
// host starts with and the presses and typing that reach the state. Never shipped.
import { local } from "./tools-fake.ts";
import type { McpToolsView } from "./tools-model.ts";

/** One thing a scene does once rendered: press the element a selector finds, press the button with these words, type
 * into the field a selector finds, or send the form. */
export type ToolsStep =
  | { click: string }
  | { words: string }
  | { type: [selector: string, text: string] }
  | { submit: true };

export type ToolsScene = { view: McpToolsView; previewRemote?: boolean; steps?: ToolsStep[] };

/** Three tools, one off; the last holds two saved values. */
export const THREE: McpToolsView = {
  control: "lawyer",
  servers: [
    local("court-diary", "/opt/homebrew/bin/npx", { args: ["-y", "court-diary-mcp"] }),
    local("filings", "/Users/rmehra/tools/filings-server", { enabled: false }),
    local("research-notes", "/usr/local/bin/uvx", {
      args: ["research-notes-mcp", "--folder", "/Users/rmehra/Notes"],
      env: [
        { name: "NOTES_TOKEN", saved: true },
        { name: "NOTES_ACCOUNT", saved: true },
      ],
    }),
  ],
};

const EMPTY: McpToolsView = { control: "lawyer", servers: [] };

const ADD: ToolsStep[] = [{ click: '[data-testid="tools-add"]' }];
const typed = (testid: string, text: string): ToolsStep => ({
  type: [`[data-testid="${testid}"]`, text],
});

/** The probe's pages, the shots' order. */
export const TOOLS_SCENES = {
  empty: { view: EMPTY },
  list: { view: THREE },
  "add-error": {
    view: EMPTY,
    steps: [
      ...ADD,
      typed("tools-name", "court-diary"),
      typed("tools-command", "npx"),
      typed("tools-args", "-y\ncourt-diary-mcp"),
      { submit: true },
    ],
  },
  "edit-saved": {
    view: THREE,
    steps: [{ click: '[data-tool-edit="research-notes"]' }],
  },
  "firm-list": { view: { ...THREE, control: "firmList" } },
  "firm-managed": { view: { ...THREE, control: "firmManaged" } },
  remote: {
    view: THREE,
    previewRemote: true,
    steps: [
      ...ADD,
      { words: "Address" },
      typed("tools-name", "case-research"),
      typed("tools-url", "https://mcp.example.com/research"),
      { click: '[data-testid="tools-add-header"]' },
      typed("tools-secret-name", "Authorization"),
      // a made-up value, shown only as the password field's dots
      typed("tools-secret-value", "fake-for-the-shot"),
    ],
  },
  // the copy review's states beyond the shots
  "empty-name": { view: EMPTY, steps: [...ADD, { submit: true }] },
  "name-taken": {
    view: EMPTY,
    steps: [
      ...ADD,
      typed("tools-name", "browser"),
      typed("tools-command", "/opt/homebrew/bin/npx"),
      { submit: true },
    ],
  },
  "name-invalid": {
    view: EMPTY,
    steps: [
      ...ADD,
      typed("tools-name", "court diary"),
      typed("tools-command", "/opt/homebrew/bin/npx"),
      { submit: true },
    ],
  },
  "url-invalid": {
    view: EMPTY,
    previewRemote: true,
    steps: [
      ...ADD,
      { words: "Address" },
      typed("tools-name", "case-research"),
      typed("tools-url", "mcp.example.com/research"),
      { submit: true },
    ],
  },
  "remove-confirm": {
    view: THREE,
    steps: [{ click: 'button[aria-label="Remove filings"]' }],
  },
} satisfies Record<string, ToolsScene>;

export type ToolsPage = keyof typeof TOOLS_SCENES;

/** The page a probe's ?tools= names, or null. */
export const toolsPageOf = (value: string | null): ToolsPage | null =>
  value !== null && Object.hasOwn(TOOLS_SCENES, value) ? (value as ToolsPage) : null;
