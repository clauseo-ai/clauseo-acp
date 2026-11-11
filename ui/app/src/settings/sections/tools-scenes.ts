// Settings › Tools' scenes (UC-169 to UC-189), for the probe's ?tools= pages and the copy review: each a view the fake
// host starts with and the presses and typing that reach the state. Never shipped.
import { local, type FakeImports, type FakeView } from "./tools-fake.ts";

/** One thing a scene does once rendered: press the element a selector finds, press the button with these words, type
 * into the field a selector finds, or send the form. */
export type ToolsStep =
  | { click: string }
  | { words: string }
  | { type: [selector: string, text: string] }
  | { submit: true };

export type ToolsScene = {
  view: FakeView;
  previewRemote?: boolean;
  imports?: FakeImports;
  steps?: ToolsStep[];
};

/** Three tools, one off; the last holds two saved values. */
export const THREE: FakeView = {
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

const EMPTY: FakeView = { control: "lawyer", servers: [] };

/** What Claude Desktop, Claude Code and Codex hold to import. */
const DESKTOP: FakeImports = {
  claudeDesktop: {
    add: [
      local("filesystem", "npx", { args: ["-y", "@modelcontextprotocol/server-filesystem"] }),
      local("legal-citations", "uvx", { args: ["legal-citations-mcp"] }),
      local("pdf-tools", "/opt/homebrew/bin/node", {
        args: ["/Users/rmehra/mcp/pdf-tools/index.js"],
      }),
    ],
    skip: [],
  },
  codex: { add: [local("docket", "npx", { args: ["-y", "docket-mcp"] })], skip: [] },
};
const LIST_IMPORTS: FakeImports = {
  claudeDesktop: {
    add: [
      local("legal-citations", "uvx", { args: ["legal-citations-mcp"] }),
      local("pdf-tools", "/opt/homebrew/bin/node", {
        args: ["/Users/rmehra/mcp/pdf-tools/index.js"],
      }),
    ],
    skip: [
      { name: "filesystem", reason: "commandNotFound", program: "npx" },
      { name: "github", reason: "unsupportedTransport" },
    ],
  },
  claudeCode: { add: [local("docket", "npx", { args: ["-y", "docket-mcp"] })], skip: [] },
};

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
      typed("tools-command", "court-diary-server"),
      { submit: true },
    ],
  },
  "edit-saved": {
    view: THREE,
    steps: [{ click: '[data-tool-edit="research-notes"]' }],
  },
  "firm-list": { view: { ...THREE, control: "firm_list" } },
  "firm-managed": { view: { ...THREE, control: "firm_managed" } },
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
  "empty-import": { view: EMPTY, imports: DESKTOP },
  "list-import": {
    view: THREE,
    imports: LIST_IMPORTS,
    steps: [{ click: '[data-testid="tools-import-claudeDesktop"]' }],
  },
  // the copy review's states beyond the shots
  "imported-one": {
    view: EMPTY,
    imports: { codex: DESKTOP.codex! },
    steps: [{ click: '[data-testid="tools-import-codex"]' }],
  },
  "import-reasons": {
    view: EMPTY,
    imports: {
      claudeCode: {
        add: [],
        skip: [
          { name: "filesystem", reason: "commandNotFound", program: "npx" },
          { name: "github", reason: "unsupportedTransport" },
          { name: "court-diary", reason: "unresolvedVariable" },
          { name: "docket", reason: "alreadyAdded" },
          { name: "notes", reason: "unreadable" },
          { name: "matter-search", reason: "notApproved" },
        ],
      },
    },
    steps: [{ click: '[data-testid="tools-import-claudeCode"]' }],
  },
  "command-invalid": {
    view: EMPTY,
    steps: [
      ...ADD,
      typed("tools-name", "court-diary"),
      typed("tools-command", "bin/server"),
      { submit: true },
    ],
  },
  "env-invalid": {
    view: EMPTY,
    steps: [
      ...ADD,
      typed("tools-name", "court-diary"),
      typed("tools-command", "npx"),
      { click: '[data-testid="tools-add-env"]' },
      typed("tools-secret-name", "DIARY-TOKEN"),
      { submit: true },
    ],
  },
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
