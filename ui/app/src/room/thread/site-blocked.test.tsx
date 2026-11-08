// @vitest-environment happy-dom
// The unsafe-site list's notice (0053) over the real room and FakeTransport: each state (blocked
// with its two allows, blocked more than once, the firm's no-override), one notice per block_id
// updated in place, the confirm naming the site, the reason and the reach, Allow sending
// allow_blocked_site with the notice's block_id and scope, Cancel sending nothing, and the
// refusals' words (override_off and blocklist.notice_expired take the buttons away; any other,
// command.invalid included, says its code's words and keeps them); blocklist.allow_not_saved
// shows the site allowed, with its words as a quiet line, never as a refusal.
import { act } from "react";
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { afterEach, expect, test, vi } from "vitest";
import type { SiteBlocked } from "../host-next.ts";
import {
  agentText,
  MATTER,
  reset,
  step,
  THREAD,
  TURN,
  userMessage,
  uuid,
} from "../../testing/stream-script.ts";
import type { ErrorCode, HostCommand } from "../../transport/protocol.ts";
import { defaultOutcome, mountRoom, type Harness } from "./harness.ts";
import { stubFonts } from "./testing.ts";

stubFonts();
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let h: Harness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
});

const BLOCK = uuid(9, 7);
const OTHER = uuid(10, 7);
const T2 = uuid(2, 4);
const OFF =
  "Your firm has turned off allowing sites on Clauseo's unsafe-site list, so this site stays blocked. Ask your IT team if you need it.";
const EXPIRED = "This notice has expired. Open the site again, then allow it from the new notice.";
const UNSAVED = errorWords("en", "blocklist.allow_not_saved");

const site = (over: Partial<SiteBlocked> = {}): SiteBlocked => ({
  block_id: BLOCK,
  matter: MATTER,
  thread: THREAD,
  turn: TURN,
  host: "invoice-paypa1.example",
  tag: "phishing",
  list_generated: Date.UTC(2026, 10, 1, 6),
  override_allowed: true,
  count: 1,
  ...over,
});

/** The open thread: TURN's exchange, ended, then T2's. */
const ENTRIES = [
  userMessage(1, 1, "Check the vendor's payment page."),
  agentText(2, 1, "The page didn't load."),
  step(3),
  { user_message: { ...userMessage(4, 2, "Try again?").user_message!, turn: T2 } },
  { agent_text: { ...agentText(5, 2, "It still didn't load.").agent_text!, turn: T2 } },
  step(6, { turn: T2 }),
];

/** The room with the thread open; `refuse` names the allow's refusal. */
const mount = async (refuse?: ErrorCode) => {
  h = await mountRoom({
    next: { parts: ["app"], query: () => ({ threads: [] }) },
    dispatch: (command: HostCommand) => {
      if (command.allow_blocked_site !== undefined && refuse !== undefined)
        throw { code: refuse, message: "scripted", retryable: false };
      return defaultOutcome(command);
    },
  });
  h.deliver([reset(ENTRIES)]);
  await h.settle();
  return h;
};

const play = async (event: SiteBlocked) => {
  await act(async () => h!.transport.siteBlocked(event));
  await h!.settle();
};

/** Presses one of a notice's allows; the confirm's chunk loads on the first press. */
const openConfirm = async (button: string) => {
  await h!.click(h!.one(button));
  await act(async () => {
    await vi.dynamicImportSettled();
  });
  await h!.settle();
};

const inBody = (id: string) => document.body.querySelector<HTMLElement>(`[data-testid="${id}"]`);
const notices = () => h!.all("site-blocked");
const answerOf = (el: HTMLElement) => el.closest("section.step-answer")?.textContent ?? "";

test("blocked with the firm allowing it: the host, the reason, the list and its date, and both allows", async () => {
  await mount();
  await play(site());
  const [notice] = notices();
  expect(notice?.dataset["state"]).toBe("blocked");
  const text = notice?.textContent ?? "";
  expect(text).toContain("Clauseo blocked \u2068invoice-paypa1.example\u2069.");
  expect(text).toContain("This site may try to steal passwords or details.");
  expect(text).toContain("It's on Clauseo's unsafe-site list.");
  expect(text).toContain("List of 1 November 2026");
  expect(text).not.toContain("times in this answer");
  expect(h!.one("site-allow-matter")?.textContent).toBe("Allow for this matter");
  expect(h!.one("site-allow-always")?.textContent).toBe("Always allow");
  // under its turn's answer, not the next one's
  expect(answerOf(notice!)).toContain("The page didn't load.");
});

test("malware says its reason; more than one block in the turn says how many", async () => {
  await mount();
  await play(site({ tag: "malware", count: 3 }));
  const text = notices()[0]?.textContent ?? "";
  expect(text).toContain("This site is linked to harmful software.");
  expect(h!.one("site-blocked-count")?.textContent).toContain("Blocked 3 times in this answer.");
});

test("where the firm forbids overriding, no buttons and the override_off words", async () => {
  await mount();
  await play(site({ override_allowed: false }));
  expect(notices()[0]?.dataset["state"]).toBe("no-override");
  expect(notices()[0]?.textContent).toContain(OFF);
  expect(h!.one("site-allow-matter")).toBeNull();
  expect(h!.one("site-allow-always")).toBeNull();
});

test("a repeat with the same block_id updates the one notice in place; another site adds its own", async () => {
  await mount();
  await play(site());
  const first = notices()[0];
  await play(site({ count: 2 }));
  await play(site({ count: 4 }));
  expect(notices()).toHaveLength(1);
  expect(notices()[0]).toBe(first);
  expect(h!.one("site-blocked-count")?.textContent).toContain("Blocked 4 times in this answer.");
  await play(site({ block_id: OTHER, host: "drop.example", turn: T2, tag: "malware" }));
  expect(notices()).toHaveLength(2);
  expect(answerOf(notices()[1]!)).toContain("It still didn't load.");
});

test("a notice for another thread shows nowhere here", async () => {
  await mount();
  await play(site({ thread: uuid(7, 1) }));
  const { turn: _turn, ...noTurn } = site({ block_id: OTHER });
  await play(noTurn);
  expect(notices()).toHaveLength(0);
});

test.each([
  [
    "site-allow-matter",
    "matter",
    "Allow \u2068invoice-paypa1.example\u2069 for this matter?",
    "Allowed for this matter",
  ],
  [
    "site-allow-always",
    "always",
    "Always allow \u2068invoice-paypa1.example\u2069?",
    "Allowed always",
  ],
] as const)(
  "%s: the confirm names the site, the reason and the reach; Allow sends the block_id and %s",
  async (button, scope, title, done) => {
    await mount();
    await play(site());
    await openConfirm(button);
    const confirm = inBody("site-allow-confirm");
    expect(confirm?.dataset["scope"]).toBe(scope);
    const text = confirm?.textContent ?? "";
    expect(text).toContain(title);
    expect(text).toContain("This site may try to steal passwords or details.");
    expect(text).toContain(
      scope === "always"
        ? "From now on, Clauseo can open this site in every matter."
        : "From now on, Clauseo can open this site in this matter.",
    );
    // Cancel first, where focus starts; Allow last
    expect(document.activeElement).toBe(inBody("site-allow-cancel"));
    expect(inBody("site-allow-ok")?.textContent).toBe("Allow");
    expect(h!.sent("allow_blocked_site")).toEqual([]);
    await h!.click(inBody("site-allow-ok"));
    await h!.settle();
    expect(h!.sent("allow_blocked_site")).toEqual([{ block_id: BLOCK, scope }]);
    const notice = notices()[0]!;
    expect(notice.dataset["state"]).toBe(`allowed-${scope}`);
    expect(notice.textContent).toContain(done);
    expect(h!.one("site-allow-matter")).toBeNull();
    expect(h!.one("site-allow-always")).toBeNull();
  },
);

test("Cancel sends nothing and leaves the notice as it was", async () => {
  await mount();
  await play(site());
  await openConfirm("site-allow-always");
  expect(inBody("site-allow-confirm")).not.toBeNull();
  await h!.click(inBody("site-allow-cancel"));
  await h!.settle();
  expect(h!.sent("allow_blocked_site")).toEqual([]);
  expect(notices()[0]?.dataset["state"]).toBe("blocked");
  expect(h!.one("site-allow-always")).not.toBeNull();
});

test.each([
  ["blocklist.override_off", "override-off", OFF, false],
  ["blocklist.notice_expired", "expired", EXPIRED, false],
  ["command.invalid", "refused", errorWords("en", "command.invalid"), true],
  ["store.io", "refused", errorWords("en", "store.io"), true],
] as const)(
  "an allow refused with %s says its words; buttons stay: %s",
  async (code, state, words, buttons) => {
    await mount(code);
    await play(site());
    await openConfirm("site-allow-matter");
    await h!.click(inBody("site-allow-ok"));
    await h!.settle();
    expect(h!.sent("allow_blocked_site")).toHaveLength(1);
    const notice = notices()[0]!;
    expect(notice.dataset["state"]).toBe(state);
    expect(notice.textContent).toContain(words);
    expect(h!.one("site-allow-matter") !== null).toBe(buttons);
    // a repeat of the notice keeps what the allow said
    await play(site({ count: 2 }));
    expect(notices()[0]?.textContent).toContain(words);
  },
);

test("the expired line is blocklist.notice_expired's words", () => {
  expect(errorWords("en", "blocklist.notice_expired")).toBe(EXPIRED);
});

test.each([
  ["site-allow-matter", "matter", "Allowed for this matter"],
  ["site-allow-always", "always", "Allowed always"],
] as const)(
  "%s answered blocklist.allow_not_saved: allowed, plus the until-Clauseo-closes line",
  async (button, scope, done) => {
    await mount("blocklist.allow_not_saved");
    await play(site());
    await openConfirm(button);
    await h!.click(inBody("site-allow-ok"));
    await h!.settle();
    expect(h!.sent("allow_blocked_site")).toEqual([{ block_id: BLOCK, scope }]);
    const notice = notices()[0]!;
    expect(notice.dataset["state"]).toBe(`allowed-${scope}`);
    expect(notice.textContent).toContain(done);
    expect(h!.one("site-blocked-unsaved")?.textContent).toBe(UNSAVED);
    expect(h!.one("site-allow-matter")).toBeNull();
    expect(h!.one("site-allow-always")).toBeNull();
    // a repeat of the notice keeps it allowed, and the line
    await play(site({ count: 2 }));
    expect(notices()[0]?.dataset["state"]).toBe(`allowed-${scope}`);
    expect(h!.one("site-blocked-unsaved")?.textContent).toBe(UNSAVED);
  },
);

test("a saved allow has no until-Clauseo-closes line", async () => {
  await mount();
  await play(site());
  await openConfirm("site-allow-matter");
  await h!.click(inBody("site-allow-ok"));
  await h!.settle();
  expect(notices()[0]?.dataset["state"]).toBe("allowed-matter");
  expect(h!.one("site-blocked-unsaved")).toBeNull();
});
