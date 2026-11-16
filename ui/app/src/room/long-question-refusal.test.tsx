// @vitest-environment happy-dom
// The long-question audit's refusal: what the room shows when the host refuses a question over today's 256 KiB cap
// (crates/protocol/src/command/validate.rs, MAX_PROMPT_TEXT_BYTES). The real host answers `command.invalid`
// (HostError::invalid_command) with Error::TooLarge's text as the message; the probe's FakeTransport applies no cap.
import { afterEach, expect, test } from "vitest";
import { uuid } from "../testing/stream-script.ts";
import type { HostError, ThreadId, ThreadSummary } from "../transport/protocol.ts";
import { defaultOutcome, mountRoom, THE_MATTER, type Harness } from "./thread/harness.ts";

let h: Harness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
});

const T1 = uuid(1, 1);
const MADE = uuid(9, 1);
const thread = (id: ThreadId, over: Partial<ThreadSummary> = {}): ThreadSummary => ({
  thread: id,
  matter: THE_MATTER.matter,
  agent: "claude-acp",
  status: "idle",
  ...over,
});

const tooLarge: HostError = {
  code: "command.invalid",
  message: "SendPrompt: prompt text in UTF-8 bytes is 304369, over the limit of 262144",
  retryable: false,
};

test("a question over the cap, refused with command.invalid: the room says the code's words and keeps the question", async () => {
  h = await mountRoom({
    threads: [thread(T1, { title: "Lease notice" })],
    dispatch: (command) => {
      if (command.create_thread !== undefined) return { thread_created: { thread: MADE } };
      if (command.send_prompt !== undefined) throw tooLarge;
      return defaultOutcome(command);
    },
  });
  await h.click(h.one("new-conversation"));
  const question = "Please advise on the following facts. ".repeat(40);
  await h.type(question);
  await h.key({ key: "Enter", keyCode: 13 });
  await h.settle();
  // the code's own words, in the composer's notice; the host's text with the sizes is for logs only, and the words stay
  const said = [...document.querySelectorAll("[role=alert], [role=status], [aria-live]")].map((n) =>
    n.textContent?.trim(),
  );
  expect(said).toContain("Clauseo couldn't take that request as it was.");
  expect(document.body.textContent).not.toContain("262144");
  expect((h.one("composer") as HTMLTextAreaElement | null)?.value).toBe(question);
  expect(h.sent("send_prompt")).toHaveLength(1);
});
