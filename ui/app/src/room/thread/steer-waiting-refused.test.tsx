// @vitest-environment happy-dom
// A waiting steer's Send now, refused by the host, in the real room over FakeTransport: the waiting row's note says the
// refusal's words, the Mac's own line (its `.mac` copy id) on the Mac and the base line elsewhere, never the host's text.
// Split from steer.test.tsx for its length (the 600-line limit).
import { act } from "react";
import { AGENTS_FOLDER_MAC, ERROR_WORDS } from "@clauseo/components/lib/words-errors.ts";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { header, reset, userMessage, uuid } from "../../testing/stream-script.ts";
import type { TurnId, UserMessageEntry, ViewEntry } from "../../transport/protocol.ts";
import { defaultOutcome, mountRoom, type Harness } from "./harness.ts";

let h: Harness | null = null;
const NOW = Date.UTC(2026, 10, 3, 17, 0);
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

const T1: TurnId = uuid(1, 4);
const AT = NOW - 29 * 60_000;

/** A user message of turn `turn`, with any of H9's fields. */
const ask = (
  order: number,
  n: number,
  text: string,
  turn: TurnId,
  over: Partial<UserMessageEntry> = {},
): ViewEntry => ({
  user_message: { ...userMessage(order, n, text).user_message!, turn, ...over },
});

test.each([
  ["mac", AGENTS_FOLDER_MAC],
  ["linux", ERROR_WORDS["matter.agents_folder"]],
] as const)(
  "a waiting steer's Send now refused as matter.agents_folder on %s says %j",
  async (platform, said) => {
    h = await mountRoom({
      platform,
      dispatch: (c) => {
        if (c.send_queued_now !== undefined)
          throw { code: "matter.agents_folder", message: "raw host text", retryable: false };
        return defaultOutcome(c);
      },
    });
    h.deliver([
      reset(
        [
          ask(1, 1, "Who signed the cheque?", T1),
          ask(2, 2, "Say who opened the account.", T1, { delivery: "steer", ts: AT }),
        ],
        header("running"),
      ),
    ]);
    await h.settle();
    await h.click(h.one("steer-waiting-send-now"));
    await act(async () => h!.store.flush());
    expect(h.one("steer-waiting-note")?.textContent).toBe(said);
    expect(h.host.textContent).not.toContain("raw host text");
  },
);
