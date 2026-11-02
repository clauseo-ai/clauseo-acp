// @vitest-environment happy-dom
// The agent's permission requests and elicitations in the room's live thread (permission-row.tsx), over FakeTransport:
// the answers and their keys from the composer, the confirm for allow_always, refusals, folded lines, and which prompt
// takes the keys.
import { act } from "react";
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { frame } from "@clauseo/components/lib/words-frame.ts";
import { th } from "@clauseo/components/lib/words-thread.ts";
import { t } from "@clauseo/components/lib/words.ts";
import { afterEach, expect, test, vi } from "vitest";
import { header, reset, TURN, upsert, uuid } from "../testing/stream-script.ts";
import type {
  HostError,
  PermissionEntry,
  PermissionOption,
  ToolCallEntry,
  ViewEntry,
} from "../transport/protocol.ts";
import { FakeTransport } from "../transport/fake-transport.ts";
import { mountRoom, type Harness } from "./thread/harness.ts";

let h: Harness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
  vi.restoreAllMocks();
});

const opt = (option_id: string, kind: PermissionOption["kind"]): PermissionOption => ({
  option_id,
  kind,
  agent_label: option_id,
});
const OPTIONS = [
  opt("opt-once", "allow_once"),
  opt("opt-always", "allow_always"),
  opt("opt-deny", "reject_once"),
  opt("opt-never", "reject_always"),
];
const KINDS = ["allow_once", "allow_always", "reject_once", "reject_always"] as const;

const request = (n: number) => uuid(n, 8);
const perm = (order: number, n: number, over: Partial<PermissionEntry> = {}): ViewEntry => ({
  permission: {
    order,
    request: request(n),
    options: OPTIONS,
    deliverable: "live",
    state: "pending",
    ...over,
  },
});
const chose = (kind: PermissionOption["kind"]): PermissionEntry["state"] => ({
  resolved: { choice: { selected: { option_id: "o", kind } }, by: "local_desktop" },
});
const callEntry = (order: number, n: number, over: Partial<ToolCallEntry> = {}): ViewEntry => ({
  tool_call: {
    order,
    tool_call: uuid(n, 7),
    turn: TURN,
    title: "Edit Notice to the bank.docx",
    kind: "edit",
    status: "pending",
    locations: [{ inside: { path: "Notice to the bank.docx" } }],
    content: [],
    ...over,
  },
});
const iso = (s: string) => `\u2068${s}\u2069`;
const hostError = (code: string): HostError =>
  ({ code, message: "refused", retryable: false }) as HostError;

/** The room with the given entries in its thread, the turn waiting on the lawyer. */
async function room(entries: ViewEntry[]): Promise<Harness> {
  const harness = await mountRoom();
  harness.deliver([reset(entries, header("needs_you"))]);
  return harness;
}

const prompt = () => h!.one("permission");
const answers = () =>
  KINDS.filter((k) => h!.one(`permission-${k}`) !== null).map((k) => `permission-${k}`);
const answered = () => h!.transport.only("answer_permission").map((c) => c.option);

test("each kind's answer stands as a button in a live row, and a click sends its option", async () => {
  h = await room([perm(3, 1)]);
  expect(prompt()?.dataset["state"]).toBe("pending");
  expect(answers()).toEqual(KINDS.map((k) => `permission-${k}`));
  await h.click(h.one("permission-reject_always"));
  expect(h.transport.only("answer_permission")).toEqual([
    expect.objectContaining({ request: request(1), option: "opt-never" }),
  ]);
});

test("Enter in the empty composer answers allow_once; the prompt stays busy until the entry folds", async () => {
  h = await room([perm(3, 1)]);
  expect(await h.key({ key: "Enter" })).toBe(true);
  expect(answered()).toEqual(["opt-once"]);
  // answered: no second answer goes, and the buttons wait
  await h.key({ key: "Enter" });
  await h.click(h.one("permission-allow_once"));
  expect(answered()).toEqual(["opt-once"]);
  h.deliver([upsert(perm(3, 1, { state: chose("allow_once") }))]);
  expect(prompt()?.dataset["state"]).toBe("resolved");
  expect(prompt()?.dataset["answer"]).toBe("allow_once");
  expect(answers()).toEqual([]);
});

test("with words in the field, Enter answers nothing and queues the words (U7, QU.9 a); Shift+Enter never answers", async () => {
  h = await room([perm(3, 1)]);
  await h.type("and the lease");
  await h.key({ key: "Enter" });
  expect(answered()).toEqual([]);
  expect(h.sent("send_prompt").map((c) => c.content)).toEqual([
    [{ text: { text: "and the lease" } }],
  ]);
  await h.type("");
  await h.key({ key: "Enter", shiftKey: true });
  expect(answered()).toEqual([]);
});

test("Escape answers reject_once", async () => {
  h = await room([perm(3, 1)]);
  await h.key({ key: "Escape" });
  expect(answered()).toEqual(["opt-deny"]);
});

test("Cmd or Ctrl with Enter answers allow_always, saying the OS asks to confirm while the call is pending", async () => {
  // a Mac: Linux has no confirm dialog to name
  vi.spyOn(FakeTransport.prototype, "displayInfo").mockResolvedValue({
    platform: "mac",
    release_date: null,
  });
  h = await room([perm(3, 1)]);
  let done: () => void = () => {};
  const spy = vi
    .spyOn(h.transport, "answerPermission")
    .mockImplementation(() => new Promise<void>((resolve) => (done = resolve)));
  await h.key({ key: "Enter", ctrlKey: true });
  expect(spy).toHaveBeenCalledWith(expect.anything(), request(1), "opt-always");
  expect(prompt()?.textContent).toContain("macOS is asking you to confirm.");
  expect(prompt()?.dataset["confirming"]).toBeDefined();
  done();
  await h.settle();
  expect(prompt()?.textContent).not.toContain("is asking you to confirm.");
  // still waiting for the entry to fold: no second answer
  await h.key({ key: "Escape" });
  expect(spy).toHaveBeenCalledTimes(1);
});

test("a confirm declined (transport.unauthorized) puts the prompt back, pending, with no error line", async () => {
  h = await room([perm(3, 1)]);
  const spy = vi
    .spyOn(h.transport, "answerPermission")
    .mockRejectedValueOnce(hostError("transport.unauthorized"));
  await h.key({ key: "Enter", metaKey: true });
  expect(prompt()?.dataset["state"]).toBe("pending");
  expect(prompt()?.textContent).not.toContain("is asking you to confirm.");
  for (const code of ["transport.unauthorized", "internal"])
    expect(prompt()?.textContent).not.toContain(errorWords("en", code));
  // and it answers again
  await h.key({ key: "Escape" });
  expect(spy).toHaveBeenLastCalledWith(expect.anything(), request(1), "opt-deny");
});

test("transport.unauthorized for any answer but Always for this matter shows its words", async () => {
  h = await room([perm(3, 1)]);
  vi.spyOn(h.transport, "answerPermission").mockRejectedValueOnce(
    hostError("transport.unauthorized"),
  );
  await h.click(h.one("permission-allow_once"));
  expect(prompt()?.dataset["state"]).toBe("pending");
  expect(prompt()?.textContent).toContain(errorWords("en", "transport.unauthorized"));
});

test("answered elsewhere first (permission.not_pending): no error line, nothing more is sent, and the entry folds", async () => {
  h = await room([perm(3, 1)]);
  const spy = vi
    .spyOn(h.transport, "answerPermission")
    .mockRejectedValue(hostError("permission.not_pending"));
  await h.key({ key: "Enter" });
  expect(spy).toHaveBeenCalledTimes(1);
  for (const code of ["permission.not_pending", "internal"])
    expect(prompt()?.textContent).not.toContain(errorWords("en", code));
  await h.key({ key: "Escape" });
  await h.click(h.one("permission-reject_once"));
  expect(spy).toHaveBeenCalledTimes(1);
  h.deliver([upsert(perm(3, 1, { state: chose("reject_once") }))]);
  expect(prompt()?.dataset["state"]).toBe("resolved");
  expect(answers()).toEqual([]);
});

test("another refusal shows its words under the answers, and the prompt can answer again", async () => {
  h = await room([perm(3, 1)]);
  const spy = vi
    .spyOn(h.transport, "answerPermission")
    .mockRejectedValueOnce(hostError("permission.unknown_option"));
  await h.click(h.one("permission-allow_once"));
  expect(prompt()?.textContent).toContain(errorWords("en", "permission.unknown_option"));
  await h.click(h.one("permission-reject_once"));
  expect(spy).toHaveBeenCalledTimes(2);
  expect(prompt()?.textContent).not.toContain(errorWords("en", "permission.unknown_option"));
});

test("plan-exit options never show; with no allow left only the rejects do", async () => {
  h = await room([
    perm(3, 1, {
      options: [
        opt("exit-plan-auto", "allow_always"),
        opt("exit-plan-clear-bypass", "allow_once"),
        opt("no", "reject_once"),
        opt("never", "reject_always"),
      ],
    }),
  ]);
  expect(answers()).toEqual(["permission-reject_once", "permission-reject_always"]);
  // Enter has no allow to give
  await h.key({ key: "Enter" });
  expect(answered()).toEqual([]);
});

test("expired, or pending where no answer can reach the agent: no buttons, and data-state stays the wire's", async () => {
  h = await room([
    perm(3, 1, { state: { expired: { reason: "turn_ended" } } }),
    perm(4, 2, { deliverable: "not_resumable" }),
  ]);
  const rows = h.all("permission");
  expect(rows.map((r) => r.dataset["state"])).toEqual(["expired", "pending"]);
  expect(rows.map((r) => r.dataset["answer"])).toEqual([undefined, undefined]);
  expect(answers()).toEqual([]);
  for (const r of rows)
    expect(r.textContent).toContain(th("en", "No longer waiting for an answer."));
  await h.key({ key: "Enter" });
  expect(answered()).toEqual([]);
});

test("cancelled: resolved, data-answer cancelled, no buttons", async () => {
  h = await room([
    perm(3, 1, { state: { resolved: { choice: "cancelled", by: "local_desktop" } } }),
  ]);
  expect(prompt()?.dataset["state"]).toBe("resolved");
  expect(prompt()?.dataset["answer"]).toBe("cancelled");
  expect(answers()).toEqual([]);
});

test("Every change is kept: for a change to files all inside the matter, not for a command or a file outside", async () => {
  const kept = t("en", "Every change is kept.");
  h = await room([
    callEntry(2, 1),
    perm(3, 1, { tool_call: uuid(1, 7) }),
    callEntry(4, 2, { kind: "execute", title: "Run ls", locations: [] }),
    perm(5, 2, { tool_call: uuid(2, 7) }),
    callEntry(6, 3, { locations: [{ outside: { display: "/etc/hosts" } }] }),
    perm(7, 3, { tool_call: uuid(3, 7) }),
  ]);
  expect(h.all("permission").map((r) => r.textContent?.includes(kept))).toEqual([
    true,
    false,
    false,
  ]);
  // no paper: Clauseo asks, with the agent's title beneath
  const second = h.all("permission")[1]!;
  expect(second.textContent).toContain(th("en", "Clauseo asks before going on."));
  expect(second.textContent).toContain("Run ls");
  expect(h.all("permission")[0]?.textContent).toContain(
    th("en", "Clauseo wants to save a new version of {paper}.", {
      paper: iso("Notice to the bank"),
    }),
  );
});

test("the folded line: saved only once the call completed, left as it was only while it didn't, and plain without a paper", async () => {
  const paper = iso("Notice to the bank");
  h = await room([
    callEntry(2, 1),
    perm(3, 1, { tool_call: uuid(1, 7), state: chose("allow_once") }),
    callEntry(4, 2),
    perm(5, 2, { tool_call: uuid(2, 7), state: chose("reject_once") }),
    perm(6, 3, { state: chose("allow_always") }),
    perm(7, 4, { state: chose("reject_always") }),
  ]);
  const said = () => h!.all("permission").map((r) => r.textContent);
  expect(said()).toEqual([
    th("en", "Allowed once."),
    th("en", "Not allowed. Clauseo left {paper} as it was.", { paper }),
    th("en", "Allowed for this matter."),
    th("en", "Not allowed for this matter."),
  ]);
  // the live row follows its tool call
  h.deliver([
    upsert(callEntry(2, 1, { status: "completed" })),
    upsert(callEntry(4, 2, { status: "completed" })),
  ]);
  expect(said().slice(0, 2)).toEqual([
    th("en", "Allowed once. Clauseo saved a new version of {paper}.", { paper }),
    th("en", "Not allowed."),
  ]);
});

test("only the oldest pending prompt takes the keys; once it folds, the next one does", async () => {
  h = await room([perm(3, 1, { deliverable: "not_resumable" }), perm(4, 2), perm(5, 3)]);
  await h.key({ key: "Escape" });
  expect(h.transport.only("answer_permission").map((c) => c.request)).toEqual([request(2)]);
  h.deliver([upsert(perm(4, 2, { state: chose("reject_once") }))]);
  await h.key({ key: "Enter" });
  expect(h.transport.only("answer_permission").map((c) => [c.request, c.option])).toEqual([
    [request(2), "opt-deny"],
    [request(3), "opt-once"],
  ]);
});

/** A keydown on an element, as the lawyer's key would land there. */
async function keyOn(el: Element | null, init: KeyboardEventInit): Promise<void> {
  if (el === null) throw new Error("nothing to press a key on");
  await act(async () => {
    el.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...init }));
  });
  await h!.settle();
}

test("once A folds B takes the keys, yet Enter on the page with words in the field answers nothing and the words stay", async () => {
  h = await room([perm(3, 1), perm(4, 2)]);
  await h.click(h.all("permission")[0]!.querySelector('[data-testid="permission-allow_once"]'));
  h.deliver([upsert(perm(3, 1, { state: chose("allow_once") }))]);
  await h.type("and the lease");
  await act(async () => (document.activeElement as HTMLElement | null)?.blur());
  expect(document.activeElement).toBe(document.body);
  await keyOn(document.body, { key: "Enter" });
  expect(h.transport.only("answer_permission").map((c) => c.request)).toEqual([request(1)]);
  expect(h.one("composer")).toHaveProperty("value", "and the lease");
  // with the field empty, the page's Enter is B's
  await h.type("");
  await keyOn(document.body, { key: "Enter" });
  expect(h.transport.only("answer_permission").map((c) => c.request)).toEqual([
    request(1),
    request(2),
  ]);
});

test("a held Enter, repeating across A's fold, answers only A", async () => {
  h = await room([perm(3, 1), perm(4, 2)]);
  await h.key({ key: "Enter" });
  await h.key({ key: "Enter", repeat: true });
  h.deliver([upsert(perm(3, 1, { state: chose("allow_once") }))]);
  expect(h.all("permission")[1]?.dataset["state"]).toBe("pending");
  await h.key({ key: "Enter", repeat: true });
  await h.key({ key: "Enter", repeat: true });
  expect(h.transport.only("answer_permission").map((c) => c.request)).toEqual([request(1)]);
});

test("after an answer focus is in the composer's field, by click and by key", async () => {
  h = await room([perm(3, 1), perm(4, 2)]);
  const field = h.one("composer");
  const once = h
    .all("permission")[0]!
    .querySelector<HTMLElement>('[data-testid="permission-allow_once"]')!;
  await act(async () => once.focus());
  await h.click(once);
  expect(document.activeElement).toBe(field);
  h.deliver([upsert(perm(3, 1, { state: chose("allow_once") }))]);
  const deny = h
    .all("permission")[1]!
    .querySelector<HTMLElement>('[data-testid="permission-reject_once"]')!;
  await act(async () => deny.focus());
  await keyOn(deny, { key: "Escape" });
  expect(h.transport.only("answer_permission").map((c) => c.option)).toEqual([
    "opt-once",
    "opt-deny",
  ]);
  expect(document.activeElement).toBe(field);
});

test("a click with focus on the page, as WebKit on macOS leaves it, puts focus in the field, so the page's Enter can't answer B", async () => {
  h = await room([perm(3, 1), perm(4, 2)]);
  const field = h.one("composer");
  await act(async () => (document.activeElement as HTMLElement | null)?.blur());
  expect(document.activeElement).toBe(document.body);
  await h.click(h.all("permission")[0]!.querySelector('[data-testid="permission-allow_once"]'));
  expect(document.activeElement).toBe(field);
  h.deliver([upsert(perm(3, 1, { state: chose("allow_once") }))]);
  expect(h.all("permission")[1]?.dataset["state"]).toBe("pending");
  // B takes the keys now; words in the field and focus blurred to the page, Enter sends nothing
  await h.type("and the lease");
  await act(async () => (document.activeElement as HTMLElement | null)?.blur());
  await keyOn(document.body, { key: "Enter" });
  expect(h.transport.only("answer_permission").map((c) => c.request)).toEqual([request(1)]);
  expect(h.one("composer")).toHaveProperty("value", "and the lease");
});

test("an elicitation: a question for you, the agent's words isolated, Stop the answer; never its URL; then only its words", async () => {
  const pending: ViewEntry = {
    elicitation: {
      order: 3,
      elicitation: uuid(1, 9),
      request: { url: { message: "Sign in to Westlaw", url: "https://example.com/login" } },
      state: "pending",
    },
  };
  h = await room([pending]);
  const row = () => h!.one("elicitation");
  expect(row()?.dataset["state"]).toBe("pending");
  expect(row()?.textContent).toContain(frame("en", "A question for you"));
  expect(row()?.querySelector("bdi")?.textContent).toBe("Sign in to Westlaw");
  expect(row()?.textContent).not.toContain("example.com");
  expect(row()?.querySelector("a")).toBeNull();
  await h.click(h.one("elicitation-stop"));
  expect(h.sent("cancel")).toEqual([{ thread: expect.any(String) }]);
  h.deliver([
    upsert({
      elicitation: {
        ...pending.elicitation!,
        state: { resolved: { outcome: "cancelled", by: "local_desktop" } },
      },
    }),
  ]);
  expect(row()?.dataset["state"]).toBe("resolved");
  expect(row()?.textContent).toBe("Sign in to Westlaw");
  expect(h.one("elicitation-stop")).toBeNull();
});
