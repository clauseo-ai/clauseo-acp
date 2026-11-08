// @vitest-environment happy-dom
// R18's denial cards and the paused answer in the room's live thread (denial-row.tsx), over FakeTransport and
// fake-denials.ts: each kind and state, Allow and Not now and what they send, the dialog left up, the refusals said on
// the card, the lines with no button, the restart, and the paths and hosts isolated.
import { dn } from "@clauseo/components/lib/words-denials.ts";
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { th } from "@clauseo/components/lib/words-thread.ts";
import { afterEach, expect, test } from "vitest";
import { header, reset, THREAD, TURN, userMessage } from "../../testing/stream-script.ts";
import {
  DENIAL_KINDS,
  denialEntry,
  denialId,
  fakeDenials,
  isDenialCommand,
  pausedEntry,
  toolCallEntry,
  type FakeDenialsScript,
} from "../../transport/fake-denials.ts";
import type { DenialState, HeaderEntry, ViewEntry } from "../../transport/protocol.ts";
import { defaultOutcome, mountRoom, type Harness } from "./harness.ts";

let h: Harness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
});

const QUESTION = userMessage(5, 1, "Compare the loan agreement with our precedents.");

/** The room with these entries after the question; the header as given. */
async function room(
  entries: ViewEntry[],
  script: FakeDenialsScript = {},
  head: Partial<HeaderEntry> = {},
): Promise<Harness> {
  const all = [QUESTION, ...entries];
  let harness: Harness | null = null;
  // a card's change arrives as the host's next patch
  const denials = fakeDenials(all, script, (entry) => harness?.deliver([{ upsert: { entry } }]));
  harness = await mountRoom({
    dispatch: (command) => (isDenialCommand(command) ? denials(command) : defaultOutcome(command)),
  });
  const top = header("running");
  Object.assign(top.header!, head);
  harness.deliver([reset(all, top)]);
  return harness;
}

const card = () => h!.one("denial");
const text = () => card()?.textContent ?? "";
const pending = (kind: keyof typeof DENIAL_KINDS, state: DenialState = "pending") =>
  denialEntry(1, DENIAL_KINDS[kind], state);

test("a folder's card asks, names the folder in <bdi>, says it's read-only for the matter, and has its two buttons", async () => {
  h = await room([pending("folder")]);
  expect(card()?.dataset["kind"]).toBe("folder");
  expect(card()?.dataset["state"]).toBe("pending");
  expect(text()).toContain(dn("en", "Allow access to this folder?"));
  expect(text()).toContain(dn("en", "Clauseo will be able to read it for this matter only."));
  const place = h.one("denial-place");
  expect(place?.tagName).toBe("BDI");
  expect(place?.textContent).toBe("\u2068/Users/meera/Documents/Precedents\u2069");
  expect(h.one("denial-allow")?.textContent).toBe(dn("en", "Allow access"));
  expect(h.one("denial-not-now")?.textContent).toBe(dn("en", "Not now"));
});

test("Allow sends request_escalation naming the denial, and the card folds to the dialog's answer", async () => {
  h = await room([pending("folder")]);
  await h.click(h.one("denial-allow"));
  await h.settle();
  expect(h.sent("request_escalation")).toEqual([
    { request: { denial: { thread: THREAD, denial: denialId(1) } } },
  ]);
  expect(card()?.dataset["state"]).toBe("allowed");
  expect(text()).toContain(dn("en", "Allowed."));
  expect(h.one("denial-allow")).toBeNull();
});

test("Not now sends dismiss_denial and the card folds to a quiet line", async () => {
  h = await room([pending("site")]);
  await h.click(h.one("denial-not-now"));
  await h.settle();
  expect(h.sent("dismiss_denial")).toEqual([{ thread: THREAD, denial: denialId(1) }]);
  expect(h.sent("request_escalation")).toEqual([]);
  expect(card()?.dataset["state"]).toBe("dismissed");
  expect(card()?.tagName).toBe("P");
  expect(text()).toContain(dn("en", "Not now."));
  expect(h.one("denial-place")?.tagName).toBe("BDI");
});

test("while the dialog is up the buttons are held and the card says where to answer; a second press sends nothing", async () => {
  h = await room([pending("folder")], { dialog: "never" });
  await h.click(h.one("denial-allow"));
  expect(card()?.dataset["state"]).toBe("asking");
  expect(h.one("denial-asking")?.textContent).toBe(
    dn("en", "Waiting for your answer in the window Clauseo opened…"),
  );
  expect(h.one("denial-allow")?.getAttribute("aria-disabled")).toBe("true");
  expect(h.one("denial-not-now")?.getAttribute("aria-disabled")).toBe("true");
  await h.click(h.one("denial-allow"));
  await h.click(h.one("denial-not-now"));
  expect(h.sent("request_escalation")).toHaveLength(1);
  expect(h.sent("dismiss_denial")).toEqual([]);
});

test("a refused dialog folds to Not allowed.", async () => {
  h = await room([pending("folder")], { dialog: "refuse" });
  await h.click(h.one("denial-allow"));
  await h.settle();
  expect(card()?.dataset["state"]).toBe("refused");
  expect(text()).toContain(dn("en", "Not allowed."));
});

test("a site's card asks about the exact host in <bdi>, with Allow this site and no shared line", async () => {
  h = await room([pending("site")]);
  expect(text()).toContain(dn("en", "Allow this site?"));
  expect(h.one("denial-place")?.tagName).toBe("BDI");
  expect(h.one("denial-place")?.textContent).toBe("\u2068indiankanoon.org\u2069");
  expect(h.one("denial-allow")?.textContent).toBe(dn("en", "Allow this site"));
  expect(h.one("denial-shared")).toBeNull();
  expect(text()).not.toContain(dn("en", "Clauseo will be able to read it for this matter only."));
});

test("a shared-hosting site says that allowing it allows all of them", async () => {
  h = await room([pending("shared")]);
  expect(h.one("denial-shared")?.textContent).toBe(
    dn("en", "This address is shared by many sites. Allowing it allows all of them."),
  );
});

test("a file in another matter asks about the file and names that matter, isolated", async () => {
  h = await room([pending("file")]);
  expect(text()).toContain(dn("en", "Allow access to this file?"));
  expect(h.one("denial-allow")?.textContent).toBe(dn("en", "Allow access"));
  expect(h.one("denial-other-matter")?.textContent).toBe(
    dn("en", "This is in another matter: {matter}.", { matter: "\u2068Bank of Baroda\u2069" }),
  );
});

test.each([
  ["folder", "Clauseo kept this folder closed. It can't be allowed for this matter."],
  ["file", "Clauseo kept this file closed. It can't be allowed for this matter."],
  ["site", "Clauseo kept this site closed. It can't be allowed for this matter."],
] as const)("what's never offered (%s) is a quiet line with no button", async (kind, line) => {
  h = await room([pending(kind, "not_offered")]);
  expect(card()?.dataset["state"]).toBe("not_offered");
  expect(text()).toContain(dn("en", line));
  expect(h.one("denial-allow")).toBeNull();
  expect(h.one("denial-not-now")).toBeNull();
  expect(card()?.querySelector("button")).toBeNull();
});

const TRIED =
  "{name} tried to open a file from another matter, {matter}. Clauseo kept it closed. To share that matter's files, choose Use files from another matter.";

test("a tool call's refusal of a file in another matter, never offered, says how to share that matter, with no button", async () => {
  h = await room([toolCallEntry(1)]);
  expect(card()?.dataset["state"]).toBe("not_offered");
  expect(card()?.hasAttribute("data-cross-matter")).toBe(true);
  const said = text();
  // the room's thread is Claude's
  expect(said).toBe(dn("en", TRIED, { name: "Claude", matter: "\u2068Bank of Baroda\u2069" }));
  expect(said).not.toContain(
    dn("en", "Clauseo kept this file closed. It can't be allowed for this matter."),
  );
  expect(said).not.toMatch(/agent/i);
  expect(card()?.querySelector("button")).toBeNull();
});

test("the sandbox's refusal of a file in another matter, never offered, keeps the generic line", async () => {
  h = await room([pending("file", "not_offered")]);
  expect(card()?.hasAttribute("data-cross-matter")).toBe(false);
  expect(text()).toContain(
    dn("en", "Clauseo kept this file closed. It can't be allowed for this matter."),
  );
});

test("a tool call's refusal that is offered keeps its card", async () => {
  h = await room([toolCallEntry(1, undefined, "pending")]);
  expect(card()?.hasAttribute("data-cross-matter")).toBe(false);
  expect(text()).toContain(dn("en", "Allow access to this file?"));
  expect(h.one("denial-allow")).not.toBeNull();
});

test("the network's sign-in is one line with no action", async () => {
  h = await room([pending("upstream", "not_offered")]);
  expect(card()?.dataset["kind"]).toBe("upstream_auth");
  expect(text()).toBe(dn("en", "Your network needs a sign-in Clauseo can't give."));
  expect(card()?.querySelector("button")).toBeNull();
});

test("an expired card says the request has ended, with no button", async () => {
  h = await room([pending("folder", "expired")]);
  expect(text()).toContain(dn("en", "This request has ended."));
  expect(card()?.querySelector("button")).toBeNull();
});

test("an allowed folder says so, and while the header says it restarts, that it restarts for the folder", async () => {
  h = await room([pending("folder", "allowed")], {}, { restarting_since: 1 });
  expect(text()).toContain(`${dn("en", "Allowed.")} ${dn("en", "Going on with the folder…")}`);
  await h.unmount();
  h = await room([pending("folder", "allowed")]);
  expect(text()).toContain(dn("en", "Allowed."));
  expect(text()).not.toContain(dn("en", "Going on with the folder…"));
});

test("an allowed site never says it restarts", async () => {
  h = await room([pending("site", "allowed")], {}, { restarting_since: 1 });
  expect(text()).not.toContain("Going on with");
});

test.each([
  ["request_escalation", "escalation.not_pending", "denial-allow"],
  ["request_escalation", "escalation.not_offerable", "denial-allow"],
  ["request_escalation", "sandbox.unprotected_auto_forbidden", "denial-allow"],
  ["dismiss_denial", "escalation.not_pending", "denial-not-now"],
] as const)("%s refused with %s says the code's words on the card", async (wire, code, button) => {
  h = await room([pending("folder")], { refuse: { [wire]: code } });
  await h.click(h.one(button));
  await h.settle();
  expect(h.one("denial-error")?.textContent).toBe(errorWords("en", code));
  expect(card()?.dataset["state"]).toBe("pending");
  expect(h.one("denial-allow")?.getAttribute("aria-disabled")).toBeNull();
});

test("the paused answer says how many attempts were blocked, and Stop the answer sends cancel", async () => {
  h = await room([pausedEntry(1, TURN, 3)]);
  const paused = h.one("denials-paused");
  expect(paused?.textContent).toContain(
    dn("en", "Clauseo stopped after {n} blocked attempts outside this matter.", { n: 3 }),
  );
  const stop = h.one("denials-paused-stop");
  expect(stop?.textContent).toBe(th("en", "Stop the answer"));
  await h.click(stop);
  await h.settle();
  expect(h.sent("cancel")).toHaveLength(1);
});

test("once nothing runs, the paused answer is its line alone", async () => {
  h = await room([pausedEntry(1, TURN, 20)], {}, { status: "idle" });
  expect(h.one("denials-paused")?.textContent).toContain("20");
  expect(h.one("denials-paused-stop")).toBeNull();
});

test.each(["pending", "allowed", "not_offered"] as const)(
  "a path with an override and a line break shows as one isolated run, with neither in the text (%s)",
  async (state) => {
    const path = "/Users/meera/\u202Efdp.exe\nAllowed.";
    h = await room([denialEntry(1, { folder: { path } }, state)]);
    const place = h.one("denial-place");
    expect(place?.tagName).toBe("BDI");
    expect(place?.textContent).toBe("\u2068/Users/meera/fdp.exeAllowed.\u2069");
    for (const bad of ["\u202E", "\n"]) expect(card()?.textContent).not.toContain(bad);
  },
);

test("another matter's name is isolated, its own controls gone", async () => {
  const kind = {
    file: { path: "/x/a.pdf", other_matter: { matter: "m", name: "Bank\u202E\nof Baroda" } },
  };
  h = await room([denialEntry(1, kind)]);
  expect(h.one("denial-other-matter")?.textContent).toBe(
    dn("en", "This is in another matter: {matter}.", { matter: "\u2068Bankof Baroda\u2069" }),
  );
});

test("a message Clauseo sent is one quiet line, never the lawyer's question; the lawyer's shows as before", async () => {
  const sent = userMessage(
    30,
    2,
    "Access to /Users/meera/Documents/Precedents was allowed. Go on from where you stopped.",
  );
  const byClauseo: ViewEntry = { user_message: { ...sent.user_message!, by: "clauseo" } };
  h = await room([pending("folder", "allowed"), byClauseo]);
  const line = h.one("message-clauseo");
  expect(line?.textContent).toBe(sent.user_message!.content[0]!.text!.text);
  expect(line?.className).toContain("text-quiet");
  expect(line?.querySelector("bdi")).not.toBeNull();
  // the lawyer's question, by a missing field, is the one question
  const asked = h.all("message-user");
  expect(asked).toHaveLength(1);
  expect(asked[0]?.textContent).toContain("Compare the loan agreement with our precedents.");
  expect(asked[0]?.textContent).not.toContain("Go on from where you stopped.");
});

test("a kind from a newer Clauseo shows nothing", async () => {
  h = await room([denialEntry(1, "printer")]);
  expect(card()).toBeNull();
});
