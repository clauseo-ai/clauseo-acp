// @vitest-environment happy-dom
// A step Clauseo's pre-write guard stopped, in the room's thread: the host's notice (held_open_blocked with its paper,
// codex_folder_blocked, guard_step_blocked, guard_steps_more, no_copy_blocked with its file) is a quiet warning line in the app's words, the agent and the paper named, never the host's
// English text. In English and Hindi. The harness thread is claude-acp; the words are the same for any agent name.
import { th } from "@clauseo/components/lib/words-thread.ts";
import { afterEach, expect, test } from "vitest";
import { reset, userMessage, uuid } from "../../testing/stream-script.ts";
import type { NoticeKind, ViewEntry } from "../../transport/protocol.ts";
import { mountRoom, type Harness } from "./harness.ts";

let h: Harness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
});

/** Papers as Versions names them: isolated, so a right-to-left mark can't reorder the line. */
const iso = (name: string) => `\u2068${name}\u2069`;

const HOST_TEXT = "The host's English fallback.";

const notice = (order: number, kind: NoticeKind): ViewEntry => ({
  notice: {
    order,
    event: uuid(order, 10),
    level: "warning",
    text: HOST_TEXT,
    kind,
    ts: order * 1000,
  },
});

for (const lang of ["en", "hi"] as const) {
  test(`the guard's stopped steps show as quiet warning lines naming the paper and the agent (${lang})`, async () => {
    h = await mountRoom({ display: { language: lang } });
    h.deliver([
      reset([
        userMessage(5, 1, "Add a clause on late payment to the lease."),
        notice(6, { held_open_blocked: { file: "Drafts/Lease.docx" } }),
        notice(7, "codex_folder_blocked"),
        notice(8, "guard_step_blocked"),
        notice(9, { guard_steps_more: { more: 3 } }),
        notice(10, { no_copy_blocked: { file: "Scan.pdf" } }),
      ]),
    ]);
    await h.settle();
    const notices = h.all("notice");
    expect(notices.map((n) => n.textContent)).toEqual([
      th(
        lang,
        "Clauseo stopped a step: {name} tried to change {file}, which is open in Word. Close it in Word, then ask again.",
        { name: "Claude", file: iso("Lease.docx") },
      ),
      th(
        lang,
        "Clauseo stopped a step: {name} tried to change its own settings in this matter's folder.",
        {
          name: "Claude",
        },
      ),
      th(lang, "Clauseo stopped a step."),
      th(lang, "Clauseo stopped {n} more steps.", { n: "3" }),
      th(
        lang,
        "Clauseo stopped a step: {name} tried to delete or replace {file}. Clauseo has no copy of it, so Undo couldn't have brought it back.",
        { name: "Claude", file: iso("Scan.pdf") },
      ),
    ]);
    expect(h.host.textContent).not.toContain(HOST_TEXT);
    expect(notices.map((n) => n.dataset["level"])).toEqual([
      "warning",
      "warning",
      "warning",
      "warning",
      "warning",
    ]);
  });
}
