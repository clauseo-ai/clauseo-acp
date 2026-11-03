// The permission prompt's props, read from the host's entry and its tool call (permission-model.ts).
import { readFileSync } from "node:fs";
import { expect, test } from "vitest";
import type { StreamView } from "../store/view.ts";
import type {
  PermissionEntry,
  PermissionOption,
  ToolCallEntry,
  ToolLocation,
} from "../transport/protocol.ts";
import {
  elicitationMessage,
  offersOf,
  PERMISSION_TEST_IDS,
  PLAN_EXIT_OPTIONS,
  paperName,
  permissionModel,
} from "./permission-model.ts";
import { keyedRequest } from "./permission-keyed.ts";

const opt = (option_id: string, kind: PermissionOption["kind"]): PermissionOption => ({
  option_id,
  kind,
  agent_label: option_id,
});
const OPTIONS = [
  opt("deny-always", "reject_always"),
  opt("allow", "allow_once"),
  opt("deny", "reject_once"),
  opt("always", "allow_always"),
];

const entry = (over: Partial<PermissionEntry> = {}): PermissionEntry => ({
  order: 3,
  request: "r1",
  options: OPTIONS,
  deliverable: "live",
  state: "pending",
  ...over,
});

const call = (over: Partial<ToolCallEntry> = {}): ToolCallEntry => ({
  order: 2,
  started_at: 0,
  tool_call: "t1",
  turn: "turn",
  title: "Edit Notice.docx",
  kind: "edit",
  status: "pending",
  locations: [{ inside: { path: "drafts/Notice to the bank.docx" } }],
  content: [],
  ...over,
});

const inside = (path: string): ToolLocation => ({ inside: { path } });
const outside = (display: string): ToolLocation => ({ outside: { display } });
const iso = (s: string) => `\u2068${s}\u2069`;

test("offers: known kinds only, no plan-exit option, the first of each kind, in ACP's kind order", () => {
  expect(offersOf(OPTIONS)).toEqual([
    { answer: "once", kind: "allow_once", option: "allow" },
    { answer: "always", kind: "allow_always", option: "always" },
    { answer: "deny", kind: "reject_once", option: "deny" },
    { answer: "never", kind: "reject_always", option: "deny-always" },
  ]);
  expect(
    offersOf([
      opt("x", "unknown"),
      opt("allow", "allow_once"),
      opt("allow-2", "allow_once"),
      opt("exit-plan-auto", "allow_always"),
    ]).map((o) => o.option),
  ).toEqual(["allow"]);
  // with every allow a plan exit, only the rejects are left
  expect(
    offersOf([
      opt("exit-plan-bypass", "allow_always"),
      opt("exit-plan-clear-auto", "allow_once"),
      opt("no", "reject_once"),
    ]).map((o) => o.answer),
  ).toEqual(["deny"]);
});

test("the plan-exit options are the bridge's MODE_OPTIONS, one list in two homes", () => {
  const rust = readFileSync(
    new URL("../../../../crates/desktop/src/bridge.rs", import.meta.url),
    "utf8",
  );
  const block = /const MODE_OPTIONS: \[&str; \d+\] = \[([^\]]*)\]/.exec(rust)?.[1];
  expect(block).toBeDefined();
  const ids = [...block!.matchAll(/"([^"]+)"/g)].map((m) => m[1]);
  expect(ids).toEqual([...PLAN_EXIT_OPTIONS]);
});

test("the paper: an edit of exactly one file inside the matter, by its name without the extension, isolated", () => {
  expect(paperName("drafts/Notice to the bank.docx")).toBe("Notice to the bank");
  expect(paperName("a\\b\\v1.final.pdf")).toBe("v1.final");
  expect(paperName(".env")).toBe(".env");
  expect(permissionModel(entry(), call()).paper).toBe(iso("Notice to the bank"));
  expect(permissionModel(entry(), call({ kind: "delete" })).paper).toBeNull();
  expect(
    permissionModel(entry(), call({ locations: [inside("a.docx"), inside("b.docx")] })).paper,
  ).toBeNull();
  expect(permissionModel(entry(), call({ locations: [outside("/etc/x.txt")] })).paper).toBeNull();
  expect(permissionModel(entry(), undefined).paper).toBeNull();
});

test("the question: the paper's, else Clauseo asks with the call's title beneath; no call, no title", () => {
  const paper = permissionModel(entry(), call());
  expect([paper.asks, paper.title]).toEqual([false, null]);
  const run = permissionModel(entry(), call({ kind: "execute", title: "Run ls", locations: [] }));
  expect([run.asks, run.title]).toEqual([true, iso("Run ls")]);
  const none = permissionModel(entry(), undefined);
  expect([none.asks, none.title]).toEqual([true, null]);
});

test("kept: an edit, delete or move with at least one place, every one inside the matter", () => {
  const kept = (c: ToolCallEntry | undefined) => permissionModel(entry(), c).kept;
  expect(kept(call())).toBe(true);
  expect(kept(call({ kind: "move", locations: [inside("a"), inside("b")] }))).toBe(true);
  expect(kept(call({ kind: "delete" }))).toBe(true);
  expect(kept(call({ kind: "execute" }))).toBe(false);
  expect(kept(call({ locations: [] }))).toBe(false);
  expect(kept(call({ locations: [inside("a"), outside("/b")] }))).toBe(false);
  expect(kept(call({ locations: ["newer"] }))).toBe(false);
  expect(kept(undefined)).toBe(false);
});

test("done: the tool call completed", () => {
  expect(permissionModel(entry(), call({ status: "completed" })).done).toBe(true);
  expect(permissionModel(entry(), call({ status: "in_progress" })).done).toBe(false);
  expect(permissionModel(entry(), undefined).done).toBe(false);
});

test("the state, and the wire's state and answer for the e2e tests", () => {
  const read = (e: PermissionEntry) => {
    const m = permissionModel(e, undefined);
    return [m.state, m.dataState, m.dataAnswer];
  };
  const chose = (kind: PermissionOption["kind"]): PermissionEntry["state"] => ({
    resolved: { choice: { selected: { option_id: "o", kind } }, by: "local_desktop" },
  });
  expect(read(entry())).toEqual(["pending", "pending", undefined]);
  expect(read(entry({ deliverable: "not_resumable" }))).toEqual(["gone", "pending", undefined]);
  expect(read(entry({ deliverable: "unknown" }))).toEqual(["gone", "pending", undefined]);
  expect(read(entry({ state: chose("allow_once") }))).toEqual(["once", "resolved", "allow_once"]);
  expect(read(entry({ state: chose("allow_always") }))).toEqual([
    "always",
    "resolved",
    "allow_always",
  ]);
  expect(read(entry({ state: chose("reject_once") }))).toEqual(["deny", "resolved", "reject_once"]);
  expect(read(entry({ state: chose("reject_always") }))).toEqual([
    "never",
    "resolved",
    "reject_always",
  ]);
  expect(read(entry({ state: chose("unknown") }))).toEqual(["gone", "resolved", "unknown"]);
  expect(
    read(entry({ state: { resolved: { choice: "cancelled", by: "local_desktop" } } })),
  ).toEqual(["gone", "resolved", "cancelled"]);
  expect(read(entry({ state: { resolved: { choice: "newer", by: "local_desktop" } } }))).toEqual([
    "gone",
    "resolved",
    undefined,
  ]);
  expect(read(entry({ state: { expired: { reason: "turn_ended" } } }))).toEqual([
    "gone",
    "expired",
    undefined,
  ]);
});

test("the test ids name each answer by the ACP kind it sends", () => {
  expect(PERMISSION_TEST_IDS.root).toBe("permission");
  expect(
    (["once", "always", "deny", "never"] as const).map((a) => PERMISSION_TEST_IDS.answer(a)),
  ).toEqual([
    "permission-allow_once",
    "permission-allow_always",
    "permission-reject_once",
    "permission-reject_always",
  ]);
});

test("the keyed request: the oldest pending one an answer can reach, read again only for a new id list", () => {
  const p = (request: string, over: Partial<PermissionEntry> = {}) => ({
    permission: entry({ request, ...over }),
  });
  const entries = new Map<string, object>([
    ["user_message:m", { user_message: {} }],
    ["permission:a", p("a", { state: { expired: { reason: "turn_ended" } } })],
    ["permission:b", p("b", { deliverable: "not_resumable" })],
    ["permission:c", p("c")],
    ["permission:d", p("d")],
  ]);
  const ids = [...entries.keys()];
  const view = { ids, entries } as unknown as StreamView;
  expect(keyedRequest(view)).toBe("c");
  entries.set("permission:c", p("c", { state: { expired: { reason: "turn_ended" } } }));
  expect(keyedRequest(view)).toBe("d");
  entries.delete("permission:d");
  expect(keyedRequest(view)).toBeNull();
  expect(keyedRequest(undefined)).toBeNull();
});

test("an elicitation's message: the form's or the link's, never its URL", () => {
  const e = (request: unknown) =>
    elicitationMessage({ order: 1, elicitation: "e", request, state: "pending" } as never);
  expect(e({ form: { message: "Your bar number?", schema: {} } })).toBe("Your bar number?");
  expect(e({ url: { message: "Sign in", url: "https://example.com" } })).toBe("Sign in");
  expect(e("newer")).toBeNull();
});
