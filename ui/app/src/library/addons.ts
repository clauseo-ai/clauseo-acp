// Stage D's skills and connectors list, as the app reads it (round24/stage-d-contract-draft1.md,
// draft 2): the answer to {"list_addons": {"matter"?: id}} and the event {"addons_changed": {}}.
// Local types, named and shaped as the contract has them, until the host's own reach bindings.ts.
// TODO(stage D, D1: list_addons + addons_changed): once bindings.ts has these types, replace this
// file's body with re-exports of them (AddonView, AddonsList, ...); nothing else changes, since
// every reader imports from here.
import type { MatterId } from "../transport/protocol.ts";

/** "all": on in every matter unless switched off there; "matter": off everywhere but the matters it
 * is switched on in. */
export type AddonScope = "all" | "matter";

/** Where an added item came from: a skill by its source, a tool (an MCP server) by how Clauseo
 * reaches it. */
export type AddonFromKind =
  | "skills_sh"
  | "github"
  | "url"
  | "zip"
  | "folder"
  | "program"
  | "address"
  | "import";

export type AddonStatus =
  | "ready"
  /** Its files changed since it was added; it stays off until turned on again. */
  | "changed"
  | "invalid"
  /** The firm's policy. */
  | "blocked"
  /** A tool's saved key is missing. */
  | "needs_secret"
  /** Clauseo Case Law, not bought and signed in yet. */
  | "needs_setup"
  /** Draft 2: Clauseo Case Law's liveness check failed; checked again when the place opens. */
  | "unreachable";

/** One item of list_addons. */
export type AddonView = {
  kind: "skill" | "tool";
  id: string;
  /** The host's name for it; the app shows its own words for a built-in. */
  name: string;
  source: "built_in" | "added";
  /** One line, at most 200 characters; may be empty for an added tool. A built-in's is the host's
   * fixed line. */
  description: string;
  /** Added items only: text is the program, the https address, the source, or "Claude Desktop" and
   * the like. */
  from?: { kind: AddonFromKind; text: string };
  scope: AddonScope;
  /** On in the matter asked about; without a matter, in the all-matters default. */
  on: boolean;
  /** Skills only: the folder holds a file an agent could run. */
  has_scripts?: boolean;
  status: AddonStatus;
  can_remove: boolean;
  /** The firm decides this item's switches. */
  locked: boolean;
  /** Draft 4, D2a: the matters whose switch differs from the scope's
   * default. */
  switched?: MatterId[];
  /** Draft 4, D2a: an added tool's server as today's McpServerView has it, names only. */
  server?: AddonServer;
  /** Draft 4, D3/D5: the host-fetched site icon or GitHub avatar, as a data: URL. The
   * app never fetches. */
  icon?: string;
};

/** A saved value's name, and whether a value is held; never the value. */
export type AddonSecretName = { name: string; saved: boolean };

/** An added tool's server (draft 4): its program and arguments, or its address. */
export type AddonServer = {
  command?: string;
  args: string[];
  url?: string;
  env: AddonSecretName[];
  headers: AddonSecretName[];
  enabled: boolean;
};

/** list_addons' answer: {"addons": AddonsList}. */
export type AddonsList = { items: AddonView[] };

/** The query. */
export type ListAddons = { list_addons: { matter?: MatterId } };

/** The app event after any change: the app reads the list again. */
export type AddonsChanged = { addons_changed: Record<string, never> };
