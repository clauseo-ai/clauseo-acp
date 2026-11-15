// @vitest-environment happy-dom
// A matter's kept permissions (kept-permissions.tsx) over FakeTransport: kept_permissions is read
// for the matter as the list opens, each rule shows in plain words by its kind, Remove sends
// remove_kept_permission with the exact rule and reads the list again, the empty list has its one
// line and a refusal its code's words. keptLine words each kind in English and Hindi.
import "@clauseo/components/lib/words-hi.ts";
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, test } from "vitest";
import { MATTER } from "../testing/stream-script.ts";
import { FakeTransport } from "../transport/fake-transport.ts";
import type { HostCommand, KeptPermission } from "../transport/protocol.ts";
import { KeptPermissions } from "./kept-permissions.tsx";
import { keptLine } from "./kept-permissions-commands.ts";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const iso = (text: string) => `\u2068${text}\u2069`;

const RUN: KeptPermission = { rule: "Bash(npm test)", kind: "run", subject: "npm test" };
const EDIT: KeptPermission = {
  rule: "Edit(//work/smith/work/out.txt)",
  kind: "edit",
  subject: "work/out.txt",
};
const READ: KeptPermission = { rule: "Read(//srv/a.txt)", kind: "read", subject: "/srv/a.txt" };
const SITE: KeptPermission = {
  rule: "WebFetch(domain:example.com)",
  kind: "site",
  subject: "example.com",
};
const TOOL: KeptPermission = {
  rule: "mcp__court__find_case",
  kind: "tool",
  subject: "find_case",
  server: "court",
};

let root: Root | null = null;

afterEach(async () => {
  await act(async () => root?.unmount());
  document.body.replaceChildren();
  root = null;
});

/** Mounts the list open on MATTER over a host keeping `kept`; a removal takes the rule out, or is
 * refused with `refuse`. */
async function mount(kept: KeptPermission[], refuse?: { code: string; message: string }) {
  let held = [...kept];
  const transport = new FakeTransport({
    query: (query) => {
      if (typeof query === "object" && "kept_permissions" in query) {
        expect(query.kept_permissions?.matter).toBe(MATTER);
        return { kept_permissions: held };
      }
      throw new Error("not scripted");
    },
    dispatch: (envelope) => {
      const removed = (envelope.command as HostCommand).remove_kept_permission;
      if (removed === undefined) throw new Error("not scripted");
      if (refuse !== undefined) throw refuse;
      held = held.filter((k) => k.rule !== removed.rule);
      return "done";
    },
  });
  const host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => {
    root?.render(<KeptPermissions transport={transport} matter={MATTER} onClose={() => {}} />);
  });
  await act(async () => {});
  return transport;
}

const rows = () =>
  [...document.querySelectorAll('[data-testid="kept-permission"]')].map((row) => row.textContent);

test("each kept rule shows in plain words, and Remove sends its exact rule and reads again", async () => {
  const transport = await mount([RUN, EDIT, SITE, TOOL]);
  expect(rows()).toEqual([
    `Run: ${iso("npm test")}Remove`,
    `Edit: ${iso("work/out.txt")}Remove`,
    `Any page on ${iso("example.com")}Remove`,
    `Use tool: ${iso("court")} ${iso("find_case")}Remove`,
  ]);
  const remove = document.querySelectorAll<HTMLButtonElement>(
    '[data-testid="kept-permission-remove"]',
  )[1];
  expect(remove?.getAttribute("aria-label")).toBe(`Remove Edit: ${iso("work/out.txt")}`);
  await act(async () => remove?.click());
  await act(async () => {});
  const sent = transport.only("dispatch").map(({ command }) => command.command);
  expect(sent).toEqual([{ remove_kept_permission: { matter: MATTER, rule: EDIT.rule } }]);
  expect(transport.only("query")).toHaveLength(2);
  expect(rows()).toHaveLength(3);
});

test("a matter that keeps nothing says so in one line", async () => {
  await mount([]);
  expect(rows()).toEqual([]);
  expect(document.querySelector('[data-testid="kept-permissions-empty"]')?.textContent).toBe(
    "Nothing is kept for this matter. Claude asks each time.",
  );
});

test("a refused removal says its code's words and keeps the row", async () => {
  await mount([RUN], { code: "internal", message: "no" });
  const remove = document.querySelector<HTMLButtonElement>(
    '[data-testid="kept-permission-remove"]',
  );
  await act(async () => remove?.click());
  await act(async () => {});
  expect(document.querySelector('[data-testid="kept-permissions-failed"]')?.textContent).toBe(
    errorWords("en", "internal"),
  );
  expect(rows()).toHaveLength(1);
});

test.each<[KeptPermission, string, string]>([
  [RUN, `Run: ${iso("npm test")}`, `चलाना: ${iso("npm test")}`],
  [EDIT, `Edit: ${iso("work/out.txt")}`, `बदलना: ${iso("work/out.txt")}`],
  [READ, `Read: ${iso("/srv/a.txt")}`, `पढ़ना: ${iso("/srv/a.txt")}`],
  [SITE, `Any page on ${iso("example.com")}`, `${iso("example.com")} का कोई भी पेज`],
  [
    TOOL,
    `Use tool: ${iso("court")} ${iso("find_case")}`,
    `टूल का इस्तेमाल: ${iso("court")} ${iso("find_case")}`,
  ],
])("keptLine words a %o rule in both languages", (kept, en, hi) => {
  expect(keptLine("en", kept)).toBe(en);
  expect(keptLine("hi", kept)).toBe(hi);
});
