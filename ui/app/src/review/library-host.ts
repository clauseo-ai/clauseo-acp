// Skills and connectors for the copy review and the shots (library L1): the real room over a
// scripted host with four matters, the first open with several papers, Clauseo Case Law in the
// state asked for on the connectors' feed, and the place's list given whole by a fixed source
// (library/addons-source.ts's provideAddons), in stage D's shapes. The list is the approved
// design's (library-design/data.ts); nothing in it is real but anthropics/skills.
import { detailed, fakeNext } from "../room/fake-next.ts";
import type { PaperSummary } from "../room/host-next.ts";
import { uuid } from "../testing/stream-script.ts";
import { FakeTransport } from "../transport/fake-transport.ts";
import type {
  ConnectorStatus,
  ConnectorsState,
  MatterSummary,
  SetupEvent,
} from "../transport/protocol.ts";
import type { AddonStatus, AddonView } from "../library/addons.ts";
import { caseLawFacts, fixedAddons, provideAddons } from "../library/addons-source.ts";

const matter = (n: number, name: string): MatterSummary => ({
  matter: uuid(n, 3),
  name,
  kind: "folder",
});

export const LIBRARY_MATTERS = [
  matter(1, "Sharma v. State of Delhi"),
  matter(2, "Mehta Textiles arbitration"),
  matter(3, "Bluebell Mills v. Union of India"),
  matter(4, "Rao Family Trust"),
] as const;

const [SHARMA, MEHTA, BLUEBELL] = LIBRARY_MATTERS.map((m) => m.matter);

const PAPERS: PaperSummary[] = [
  "FIR 214 of 2025.pdf",
  "Charge sheet.pdf",
  "Bail order, Sessions Court.pdf",
  "Witness statements.pdf",
  "Draft bail application.docx",
].map((path, i) => ({ file: uuid(i + 1, 8), path, size: 120_000, modified: 1_730_000_000_000 }));

const svg = (body: string) =>
  `data:image/svg+xml,${encodeURIComponent(`<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 40 40">${body}</svg>`)}`;

/** An invented GitHub-style avatar: a 5 by 5 pattern mirrored down the middle, from the owner's
 * name. */
function avatar(owner: string, hue: number): string {
  let h = 0;
  for (const c of owner) h = (h * 31 + c.charCodeAt(0)) >>> 0;
  let cells = "";
  for (let y = 0; y < 5; y++)
    for (let x = 0; x < 3; x++) {
      if (((h >> (y * 3 + x)) & 1) === 0) continue;
      for (const cx of x === 2 ? [2] : [x, 4 - x])
        cells += `<rect x="${5 + cx * 6}" y="${5 + y * 6}" width="6" height="6"/>`;
    }
  return svg(
    `<rect width="40" height="40" fill="#f0eee9"/><g fill="hsl(${hue} 45% 48%)">${cells}</g>`,
  );
}

const RAO_ICON = svg(
  '<rect width="40" height="40" rx="6" fill="#1f4d3a"/><text x="20" y="28" font-family="Georgia,serif" font-size="22" fill="#f4efe4" text-anchor="middle">R</text>',
);

/** An added tool's server, its saved values held. */
const server = (s: {
  command?: string;
  args?: string[];
  url?: string;
  env?: string[];
  headers?: string[];
}) => ({
  ...(s.command === undefined ? {} : { command: s.command }),
  args: s.args ?? [],
  ...(s.url === undefined ? {} : { url: s.url }),
  env: (s.env ?? []).map((name) => ({ name, saved: true })),
  headers: (s.headers ?? []).map((name) => ({ name, saved: true })),
  enabled: true,
});

type Patch = Partial<AddonView> & Pick<AddonView, "id" | "name">;
const base = (
  kind: AddonView["kind"],
  source: AddonView["source"],
): Omit<AddonView, "id" | "name"> => ({
  kind,
  source,
  description: "",
  scope: "all",
  on: true,
  status: "ready",
  can_remove: source === "added",
  locked: false,
});
const skill = (p: Patch, source: AddonView["source"] = "added"): AddonView => ({
  ...base("skill", source),
  ...p,
});
const tool = (p: Patch, source: AddonView["source"] = "added"): AddonView => ({
  ...base("tool", source),
  ...p,
});

/** The list, Case Law in `caseLaw`'s status; `added` false for a lawyer who has added nothing yet.
 */
export function libraryItems(caseLaw: AddonStatus, added = true): AddonView[] {
  const own = [
    skill({
      id: "notice-drafting",
      name: "Notice drafting",
      description:
        "Drafts legal notices in the chamber's format, with the client's details taken from the matter.",
      from: { kind: "folder", text: "~/Documents/Skills/notice-drafting" },
      scope: { matter: SHARMA! },
      switched: [SHARMA!],
      has_scripts: false,
    }),
    skill({
      id: "pdf",
      name: "PDF",
      description: "Reads, fills and merges PDF files, and pulls tables out of them.",
      from: { kind: "skills_sh", text: "anthropics/skills/pdf" },
      icon: avatar("anthropics", 18),
      has_scripts: true,
    }),
    skill({
      id: "cheque-bounce",
      name: "Cheque bounce replies",
      description: "Replies to demand notices under section 138 of the Negotiable Instruments Act.",
      from: { kind: "github", text: "github.com/lexdesk/skills" },
      icon: avatar("lexdesk", 210),
      on: false,
    }),
    skill({
      id: "contract-review",
      name: "Contract review",
      description: "Marks up commercial contracts against a playbook.",
      from: { kind: "url", text: "https://skills.lexpractice.in/contract-review.zip" },
      status: "blocked",
      on: false,
    }),
    skill({
      id: "lease-review",
      name: "Lease review",
      description: "Reviews leave and licence agreements against the chamber's checklist.",
      from: { kind: "folder", text: "~/Documents/Skills/lease-review" },
      status: "changed",
      on: false,
    }),
    skill({
      id: "old-briefs",
      name: "Old briefs",
      from: { kind: "zip", text: "~/Downloads/old-briefs.zip" },
      status: "invalid",
      on: false,
    }),
  ];
  const tools = [
    tool({
      id: "court-diary",
      name: "court-diary",
      from: { kind: "program", text: "npx" },
      server: server({ command: "npx", args: ["-y", "court-diary-mcp"], env: ["DIARY_TOKEN"] }),
      scope: { matter: SHARMA! },
      switched: [SHARMA!, MEHTA!],
    }),
    tool({
      id: "precedents",
      name: "firm-precedents",
      from: { kind: "address", text: "https://precedents.raoandco.in/mcp" },
      icon: RAO_ICON,
      server: server({ url: "https://precedents.raoandco.in/mcp", headers: ["Authorization"] }),
      status: "needs_secret",
    }),
    tool({
      id: "gazette",
      name: "gazette-notifications",
      from: { kind: "address", text: "https://mcp.gazettewatch.in" },
      scope: { matter: BLUEBELL! },
      switched: [BLUEBELL!],
    }),
    tool({
      id: "research-notes",
      name: "research-notes",
      from: { kind: "import", text: "Claude Desktop" },
    }),
  ];
  return [
    skill({ id: "legal-drafting", name: "legal-drafting" }, "built_in"),
    skill({ id: "legal-research", name: "legal-research" }, "built_in"),
    skill({ id: "matter-brief", name: "matter-brief" }, "built_in"),
    skill({ id: "stetdoc", name: "stetdoc" }, "built_in"),
    ...(added ? own : []),
    tool({ id: "case-law", name: "case-law", status: caseLaw }, "built_in"),
    tool({ id: "browser", name: "browser" }, "built_in"),
    ...(added ? tools : []),
  ];
}

type CaseLawState =
  | "needs_setup"
  | "ready"
  | "out"
  | "unreachable"
  | "not_offered"
  | "off"
  | "ended";

const CONNECTOR: Record<CaseLawState, ConnectorStatus | null> = {
  needs_setup: "not_added",
  ready: { signed_in: { low: false, balance: { credits: 1_240 } } },
  out: { out_of_credit: {} },
  unreachable: null,
  not_offered: null,
  off: "not_added",
  ended: "signed_out",
};

const connectorsOf = (status: ConnectorStatus | null, available = true): ConnectorsState => ({
  available,
  locked: false,
  connectors:
    status === null
      ? []
      : [
          {
            id: "case-law",
            name: "Indian Kanoon",
            price_paise: 23_600,
            packs: [{ pack: "p2000", credits: 2_000, paise: 23_600 }],
            status,
          },
        ],
});

/** Both plans signed in, so the words say "the assistant". */
const BOTH: readonly SetupEvent[] = (["claude-acp", "codex-acp"] as const).flatMap((agent) => [
  { agent_install_progress: { agent, state: { ready: { version: "0.1.0" } } } },
  { sign_in_changed: { agent, state: { signed_in: { method: "plan", source: "own", at: 0 } } } },
]);

export type LibraryState = {
  caseLaw?: CaseLawState;
  added?: boolean;
  both?: boolean;
  /** The host lists no price (no live price list). */
  noPrice?: boolean;
};

/** The room over the library's host, its list given before the room mounts. */
export function libraryHost(state: LibraryState = {}): FakeTransport {
  const caseLaw = state.caseLaw ?? "needs_setup";
  const connectors = connectorsOf(CONNECTOR[caseLaw], caseLaw !== "off");
  const transport: FakeTransport = new FakeTransport({
    next: {
      ...fakeNext({
        matters: () => LIBRARY_MATTERS.map((m, i) => detailed(m, { last_opened: 4 - i })),
        papers: PAPERS,
      }),
      first: () => [{ connectors_changed: connectors }],
    },
    ...(state.both === true ? { setup: { first: () => BOTH } } : {}),
    openSettings: () => {},
  });
  const status: AddonStatus =
    caseLaw === "out"
      ? "needs_credit"
      : caseLaw === "ended"
        ? "signed_out"
        : caseLaw === "off"
          ? "needs_setup"
          : caseLaw;
  provideAddons(
    transport,
    fixedAddons(libraryItems(status, state.added ?? true), {
      ...caseLawFacts(connectors, true),
      ...(state.noPrice === true ? { pack: null } : {}),
    }),
  );
  return transport;
}
