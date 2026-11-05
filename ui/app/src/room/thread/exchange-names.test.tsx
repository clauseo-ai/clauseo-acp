// @vitest-environment happy-dom
// Each exchange's sections are named with their times, so no two exchanges share a landmark's name (axe's
// landmark-unique): "Your question, 4:31 pm" from the question's time, "Clauseo's answer, 4:33 pm" from its step's,
// and the plain "Clauseo's answer" while the answer runs and has no step yet. In Hindi too. Two exchanges in one minute
// would share the names: the later one's sections go unnamed, so they are no landmarks and axe's landmark-unique passes.
import axe from "axe-core";
import { th } from "@clauseo/components/lib/words-thread.ts";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import {
  agentText,
  header,
  reset,
  step,
  TURN,
  userMessage,
  uuid,
} from "../../testing/stream-script.ts";
import type { TurnId, ViewEntry } from "../../transport/protocol.ts";
import { clockTime } from "./clock.ts";
import { mountRoom, type Harness } from "./harness.ts";

let h: Harness | null = null;
const NOW = Date.UTC(2026, 10, 3, 17, 0); // a fixed instant; fixture times derive from NOW
beforeEach(() => {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(NOW);
});
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
  vi.useRealTimers();
});

const T1 = TURN;
const T2: TurnId = uuid(2, 4);
const T3: TurnId = uuid(3, 4);
/** NOW's day, at the minute given past 4 pm. */
const pm = (minute: number) => NOW - 60 * 60_000 + minute * 60_000;

const ask = (order: number, n: number, text: string, turn: TurnId, ts: number): ViewEntry => ({
  user_message: { ...userMessage(order, n, text).user_message!, turn, ts },
});
const say = (order: number, n: number, text: string, turn: TurnId): ViewEntry => ({
  agent_text: { ...agentText(order, n, text).agent_text!, turn },
});

const THREAD = [
  ask(1, 1, "Who signed the cheque?", T1, pm(28)),
  say(2, 1, "Mr. Mehta signed it.", T1),
  step(3, { turn: T1, ts: pm(29) }),
  ask(4, 2, "And who opened the account?", T2, pm(31)),
  say(5, 2, "His brother did.", T2),
  step(6, { turn: T2, ts: pm(33) }),
  ask(7, 3, "When?", T3, pm(35)),
  say(8, 3, "On 3 May", T3),
];

const names = () =>
  [...h!.host.querySelectorAll(".room-thread section")].map((s) => s.getAttribute("aria-label"));

for (const lang of ["en", "hi"] as const) {
  test(`each exchange's sections carry their own times, the running answer the plain name (${lang})`, async () => {
    h = await mountRoom({ display: { language: lang } });
    h.deliver([reset(THREAD, header("running"))]);
    await h.settle();
    const at = (minute: number) => ({ time: clockTime(lang, pm(minute)) });
    expect(names()).toEqual([
      th(lang, "Your question, {time}", at(28)),
      th(lang, "Clauseo's answer, {time}", at(29)),
      th(lang, "Your question, {time}", at(31)),
      th(lang, "Clauseo's answer, {time}", at(33)),
      th(lang, "Your question, {time}", at(35)),
      th(lang, "Clauseo's answer"),
    ]);
    expect(new Set(names()).size).toBe(6);
  });
}

test("a second exchange in the same minute leaves its sections unnamed, and axe's landmark-unique passes", async () => {
  h = await mountRoom();
  h.deliver([
    reset(
      [
        ask(1, 1, "Who signed the cheque?", T1, pm(28)),
        say(2, 1, "Mr. Mehta signed it.", T1),
        step(3, { turn: T1, ts: pm(28) + 10_000 }),
        ask(4, 2, "And who opened the account?", T2, pm(28) + 20_000),
        say(5, 2, "His brother did.", T2),
        step(6, { turn: T2, ts: pm(28) + 40_000 }),
        ask(7, 3, "When?", T3, pm(31)),
        say(8, 3, "On 3 May", T3),
        step(9, { turn: T3, ts: pm(32) }),
      ],
      header("idle"),
    ),
  ]);
  await h.settle();
  const at = (minute: number) => ({ time: clockTime("en", pm(minute)) });
  expect(names()).toEqual([
    th("en", "Your question, {time}", at(28)),
    th("en", "Clauseo's answer, {time}", at(28)),
    null,
    null,
    th("en", "Your question, {time}", at(31)),
    th("en", "Clauseo's answer, {time}", at(32)),
  ]);
  // an unnamed section is no region: the landmarks are the named ones alone, each once
  const results = await axe.run(h.host, {
    resultTypes: ["violations"],
    runOnly: ["landmark-unique"],
  });
  expect(results.violations.map((v) => v.id)).toEqual([]);
  const regions = h.host.querySelectorAll(".room-thread section[aria-label]");
  expect(regions).toHaveLength(4);
}, 30_000);
