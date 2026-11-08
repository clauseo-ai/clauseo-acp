// "Copy details for IT"'s notes (it-details.ts, blocklist/it-details.ts): which causes offer it, and
// what each note holds. Each cause fixture carries a matter's path and name beside its reason, as a
// newer host's might, and no note may copy them: only the code, the reason, the site and Clauseo's
// version go in, then the case's section of Clauseo for firm IT.
import { loadHindi } from "@clauseo/components/lib/words-core.ts";
import { firmItLink } from "@clauseo/components/lib/words-first-launch.ts";
import { expect, test } from "vitest";
import { blocklistItDetails } from "../blocklist/it-details.ts";
import { causeOf } from "./causes.ts";
import { itDetails } from "./it-details.ts";
import type { NotProtectedCause } from "../../transport/protocol.ts";

const PATH = "C:\\Users\\asha\\Matters\\Sharma v. State of Delhi\\brief.docx";
const NAME = "Sharma v. State of Delhi";
const blocked = (reason: string) =>
  causeOf({ blocked: { reason, path: PATH, matter: NAME } } as unknown as NotProtectedCause);
const plain = (name: string) => causeOf(name as NotProtectedCause);
const V = "3.0.4";
const LEAD = "Clauseo's Safe workspace is blocked on this computer.";
const DOC = "https://github.com/clauseo-ai/clauseo-acp/blob/main/docs/firm-it.md";
/** The note's last line: its case's section of Clauseo for firm IT. */
const link = (anchor: string) => `\nClauseo for firm IT: ${DOC}#${anchor}`;

const clean = (note: string | null) => {
  expect(note).not.toBeNull();
  expect(note).not.toContain(PATH);
  expect(note).not.toContain("Matters");
  expect(note).not.toContain(NAME);
  expect(note).not.toContain("asha");
};

test("blocked: policy's note is the lead, the reason and the version", () => {
  const note = itDetails.blocked("en", blocked("policy"), V);
  expect(note).toBe(`${LEAD}\nReason: policy\nClauseo version: ${V}${link("sandbox-policy")}`);
  clean(note);
});

test.each([
  ["logon_right", blocked("logon_right"), "logon_right", "sandbox-logon-right"],
  ["other_account", plain("other_account"), "other_account", "sandbox-other-account"],
  ["a reason from a newer Clauseo", blocked("a_newer_reason"), "a_newer_reason", "sandbox-setup"],
  ["a reason that isn't a name", blocked("C:\\x y"), "unknown", "sandbox-setup"],
])("blocked: %s offers it, with its reason and its section", (_c, cause, reason, anchor) => {
  expect(itDetails.offered(cause)).toBe(true);
  const note = itDetails.blocked("en", cause, V);
  expect(note).toBe(`${LEAD}\nReason: ${reason}\nClauseo version: ${V}${link(anchor)}`);
  clean(note);
});

test.each(["tool_missing", "admin_copy_missing", "admin_copy_changed", "admin_copy_unsafe"])(
  "blocked: %s (stage C's self-fixes) offers nothing",
  (reason) => {
    expect(itDetails.offered(blocked(reason))).toBe(false);
    expect(itDetails.blocked("en", blocked(reason), V)).toBeNull();
    expect(itDetails.required("en", blocked(reason), V)).toBeNull();
  },
);

test("blocked: a cause that names no block offers nothing", () => {
  expect(itDetails.blocked("en", null, V)).toBeNull();
  expect(itDetails.blocked("en", plain("needs_set_up"), V)).toBeNull();
});

test("required: a firm-only block copies sandbox.required and its reason", () => {
  const note = itDetails.required("en", blocked("policy"), V);
  expect(note).toBe(
    `${LEAD}\nCode: sandbox.required\nReason: policy\nClauseo version: ${V}${link("sandbox-policy")}`,
  );
  clean(note);
});

test("required: a computer that can't provide it copies why; a lawyer's own step copies nothing", () => {
  const unsupported = causeOf({ unsupported: { why: "windows_arm" } } as NotProtectedCause);
  expect(itDetails.required("en", unsupported, V)).toBe(
    "The firm requires Clauseo's Safe workspace, and this computer can't provide it.\n" +
      `Code: sandbox.required\nReason: windows_arm\nClauseo version: ${V}${link("sandbox-required")}`,
  );
  expect(itDetails.required("en", plain("some_newer_cause"), V)).toBe(
    "The firm requires Clauseo's Safe workspace, and this computer can't provide it.\n" +
      `Code: sandbox.required\nClauseo version: ${V}${link("sandbox-required")}`,
  );
  const notBuilt = causeOf({ unsupported: { why: "not_built" } } as NotProtectedCause);
  const pool = causeOf({ pool_full: { size: 8 } } as unknown as NotProtectedCause);
  for (const cause of [plain("needs_set_up"), plain("set_up_failed"), notBuilt, pool])
    expect(itDetails.required("en", cause, V)).toBeNull();
});

test("refused: sandbox.blocked copies its code, and the cause's firm-only reason when it names one", () => {
  expect(itDetails.refused("en", "sandbox.blocked", plain("set_up_failed"), V)).toBe(
    `${LEAD}\nCode: sandbox.blocked\nClauseo version: ${V}${link("sandbox-setup")}`,
  );
  const note = itDetails.refused("en", "sandbox.blocked", blocked("logon_right"), V);
  expect(note).toBe(
    `${LEAD}\nCode: sandbox.blocked\nReason: logon_right\nClauseo version: ${V}${link("sandbox-logon-right")}`,
  );
  clean(note);
  expect(itDetails.refused("en", "sandbox.setup_failed", blocked("policy"), V)).toBeNull();
  expect(itDetails.refused("en", null, blocked("policy"), V)).toBeNull();
});

test("the notes are in the page's language, as UD1-15's is; the codes stay as the host wrote them", async () => {
  await loadHindi();
  expect(itDetails.blocked("hi", blocked("policy"), V)).toBe(
    `इस कंप्यूटर पर Clauseo का सुरक्षित कार्यक्षेत्र अवरुद्ध है।\nकारण: policy\nClauseo संस्करण: ${V}\nफ़र्म IT के लिए Clauseo: ${DOC}#sandbox-policy`,
  );
});

test("blocklist: override_off copies the code, the site and the version", () => {
  expect(blocklistItDetails.overrideOff("en", "invoice-paypa1.example", V)).toBe(
    "Clauseo blocked a site on its unsafe-site list, and the firm's policy has turned off allowing it.\n" +
      `Code: blocklist.override_off\nSite: invoice-paypa1.example\nClauseo version: ${V}${link("unsafe-site-override")}`,
  );
});

test("firmItLink: each case's section; any other code, a newer one included, the set-up's", () => {
  for (const [code, anchor] of [
    ["policy", "sandbox-policy"],
    ["logon_right", "sandbox-logon-right"],
    ["other_account", "sandbox-other-account"],
    ["sandbox.blocked", "sandbox-setup"],
    ["sandbox.required", "sandbox-required"],
    ["blocklist.override_off", "unsafe-site-override"],
    ["tool_missing", "sandbox-tool-missing"],
    ["admin_copy_missing", "sandbox-admin-copy"],
    ["admin_copy_changed", "sandbox-admin-copy"],
    ["admin_copy_unsafe", "sandbox-admin-copy"],
    ["a_reason_from_a_newer_clauseo", "sandbox-setup"],
    ["toString", "sandbox-setup"],
  ])
    expect(firmItLink(code!)).toBe(`${DOC}#${anchor}`);
});
