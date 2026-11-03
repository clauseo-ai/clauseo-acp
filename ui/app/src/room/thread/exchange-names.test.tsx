// @vitest-environment happy-dom
// Each exchange's sections are named with their times, so no two exchanges share a landmark's name (axe's
// landmark-unique): "Your question, 4:31 pm" from the question's time, "Clauseo's answer, 4:33 pm" from its step's,
// and the plain "Clauseo's answer" while the answer runs and has no step yet. In Hindi too.
import { th } from "@clauseo/components/lib/words-thread.ts";
import { afterEach, expect, test } from "vitest";
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
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
});

const T1 = TURN;
const T2: TurnId = uuid(2, 4);
const T3: TurnId = uuid(3, 4);
/** Today on the local clock, at the minute given past 4 pm. */
const pm = (minute: number) => new Date(new Date().setHours(16, minute, 0, 0)).getTime();

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
