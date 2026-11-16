// @vitest-environment happy-dom
// "Copy details for IT" on the room's firm-only Safe workspace lines, only while the firm's policy is
// active: the Not protected note of a block only the firm can act on (here a policy that turns
// unprotected auto off; without a policy, nothing), the firm's notice under sandbox.required (a firm-only block, or a
// sandbox.blocked refusal of its set-up), and none for stage C's self-fixes. A press copies the note
// (it-details.ts) and the button says it copied; a refused clipboard shows the note to copy by hand.
// The matter's cause carries a matter path beside its reason, and the note never copies it.
import { act } from "react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { MATTER } from "../../testing/stream-script.ts";
import type { Protection, ProtectionView } from "../../transport/protocol.ts";
import { APP_VERSION } from "../../settings/version.ts";
import { mountRoom, THE_MATTER, type Harness } from "../thread/harness.ts";
import { NO_PROMPT, np, PROT, view } from "./fixtures.ts";

const PATH = "C:\\Users\\asha\\Matters\\Sharma v. State of Delhi\\brief.docx";
const POLICY = np({ blocked: { reason: "policy", path: PATH } });
const TOOL = np({ blocked: { reason: "tool_missing", path: PATH } });
const LEAD = "Clauseo's Safe workspace is blocked on this computer.";
/** A firm's policy, active: `sandbox.allow_unprotected_auto` off (firmPolicy, blocked.ts). */
const FIRM: Partial<ProtectionView> = { unprotected_auto_allowed: false };

let h: Harness | null = null;
let writeText = vi.fn(async (_text: string) => {});
beforeEach(() => {
  localStorage.setItem(
    "clauseo.protection-explained",
    JSON.stringify({ computer: true, matters: [MATTER] }),
  );
  writeText = vi.fn(async (_text: string) => {});
  Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
});
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
  Reflect.deleteProperty(navigator, "clipboard");
});

type Answer = { resolve: () => void; reject: (e: unknown) => void };

async function mount(protection: Protection, over: Partial<ProtectionView> = {}) {
  const answers: Answer[] = [];
  h = await mountRoom({
    matters: [{ ...THE_MATTER, protection }],
    next: { parts: ["app"], query: () => ({ threads: [] }) },
    protection: {
      view: () => view(protection, over),
      command: () => new Promise<void>((resolve, reject) => answers.push({ resolve, reject })),
    },
  });
  await settle();
  return answers;
}
const settle = async () => {
  for (let i = 0; i < 5; i += 1) await h!.settle();
};
const inBody = (id: string) => document.body.querySelector<HTMLElement>(`[data-testid="${id}"]`);
const openNote = async () => {
  await h!.click(h!.one("workspace-line"));
  await settle();
};
const pressIt = async () => {
  await h!.click(inBody("protection-it"));
  await settle();
};
/** The one note copied: never the matter's path or name. */
const copied = (): string => {
  expect(writeText).toHaveBeenCalledTimes(1);
  const note = writeText.mock.calls[0]![0];
  expect(note).not.toContain(PATH);
  expect(note).not.toContain(THE_MATTER.name);
  return note;
};

test("the note of a policy block offers it under a firm's policy; a press copies the reason and the version, then says copied", async () => {
  await mount(POLICY, FIRM);
  await openNote();
  expect(inBody("protection-it")?.textContent).toBe("Copy details for IT");
  await pressIt();
  expect(copied()).toBe(
    `${LEAD}\nReason: policy\nClauseo version: ${APP_VERSION}\nClauseo for firm IT: https://github.com/clauseo-ai/clauseo-acp/blob/main/docs/firm-it.md#sandbox-policy`,
  );
  expect(inBody("protection-it")?.textContent).toBe("Message copied");
});

test.each([
  ["other_account", PROT.otherAccount, "other_account", "sandbox-other-account"],
  ["logon_right", PROT.logon, "logon_right", "sandbox-logon-right"],
  [
    "a reason from a newer Clauseo",
    PROT.blockedNewer,
    "a_reason_from_a_newer_clauseo",
    "sandbox-setup",
  ],
])(
  "the note of %s offers it, with its reason, ending with its section",
  async (_c, p, reason, anchor) => {
    await mount(p, FIRM);
    await openNote();
    await pressIt();
    const note = copied();
    expect(note).toContain(`\nReason: ${reason}\n`);
    expect(
      note.endsWith(
        `\nClauseo for firm IT: https://github.com/clauseo-ai/clauseo-acp/blob/main/docs/firm-it.md#${anchor}`,
      ),
    ).toBe(true);
  },
);

test.each([
  ["tool_missing", TOOL],
  ["admin_copy_missing", PROT.missing],
  ["needs_set_up", PROT.needs],
])("the note of %s offers nothing for IT", async (_c, p) => {
  await mount(p, FIRM);
  await openNote();
  expect(inBody("np-note")).not.toBeNull();
  expect(inBody("protection-it")).toBeNull();
});

test.each([
  ["a policy block", POLICY],
  ["other_account", PROT.otherAccount],
  ["logon_right", PROT.logon],
  ["a reason from a newer Clauseo", PROT.blockedNewer],
])("without a firm's policy, the note of %s offers nothing for IT", async (_c, p) => {
  await mount(p);
  await openNote();
  expect(inBody("np-note")).not.toBeNull();
  expect(inBody("protection-it")).toBeNull();
  expect(inBody("np-note")?.textContent).not.toMatch(/\bIT\b|[Yy]our firm/);
});

test("the firm's notice over a policy block copies sandbox.required and the reason", async () => {
  await mount(POLICY, { required: true });
  expect(h!.one("protection-required")).not.toBeNull();
  await pressIt();
  expect(copied()).toBe(
    `${LEAD}\nCode: sandbox.required\nReason: policy\nClauseo version: ${APP_VERSION}\nClauseo for firm IT: https://github.com/clauseo-ai/clauseo-acp/blob/main/docs/firm-it.md#sandbox-policy`,
  );
});

test("the firm's notice offers it once its set-up is refused with sandbox.blocked", async () => {
  const answers = await mount(PROT.needs, { required: true, ...NO_PROMPT });
  expect(inBody("protection-it")).toBeNull();
  await h!.click(h!.one("protection-setup"));
  await settle();
  await act(async () =>
    answers[0]!.reject({ code: "sandbox.blocked", message: "no", retryable: false }),
  );
  await settle();
  await pressIt();
  expect(copied()).toBe(
    `${LEAD}\nCode: sandbox.blocked\nClauseo version: ${APP_VERSION}\nClauseo for firm IT: https://github.com/clauseo-ai/clauseo-acp/blob/main/docs/firm-it.md#sandbox-setup`,
  );
});

test("a refused clipboard shows the note to copy by hand, and nothing says copied", async () => {
  writeText.mockImplementation(() => Promise.reject(new Error("NotAllowedError")));
  await mount(POLICY, FIRM);
  await openNote();
  await pressIt();
  expect(inBody("protection-it")?.textContent).toBe("Copy details for IT");
  const field = inBody("protection-it-text") as HTMLTextAreaElement | null;
  expect(field?.value).toBe(
    `${LEAD}\nReason: policy\nClauseo version: ${APP_VERSION}\nClauseo for firm IT: https://github.com/clauseo-ai/clauseo-acp/blob/main/docs/firm-it.md#sandbox-policy`,
  );
  expect(field?.value).not.toContain(PATH);
});
