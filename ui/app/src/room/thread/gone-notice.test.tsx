// @vitest-environment happy-dom
// Files deleted during a step that Clauseo kept no copy of, in the room's thread: the host's files_gone_no_copy notice
// is a quiet warning line in the app's words, each file named with why there's no copy, the rest counted, never the
// host's English text. In English and Hindi.
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

/** Files as Versions names them: isolated, so a right-to-left mark can't reorder the line. */
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
  test(`files gone with no copy show as one quiet warning line, each named with why (${lang})`, async () => {
    h = await mountRoom({ display: { language: lang } });
    h.deliver([
      reset([
        userMessage(5, 1, "Tidy up the scans folder."),
        notice(6, {
          files_gone_no_copy: {
            files: [
              { path: "Drafts/Brief.docx", reason: "cloud_only" },
              { path: "Scans/Photos.zip", reason: "over_size_cap" },
            ],
            more: 2,
          },
        }),
      ]),
    ]);
    await h.settle();
    const notices = h.all("notice");
    expect(notices.map((n) => n.textContent)).toEqual([
      [
        th(
          lang,
          "{file} was deleted during this answer. Clauseo had no copy of it to bring back. It was kept only in your cloud service, which may still have it among its deleted files.",
          { file: iso("Brief.docx") },
        ),
        th(
          lang,
          "{file} was deleted during this answer. It was too large for Clauseo to keep a copy of, so Clauseo can't bring it back.",
          { file: iso("Photos.zip") },
        ),
        th(lang, "{n} more files Clauseo had no copy of were deleted during this answer.", {
          n: 2,
        }),
      ].join(" "),
    ]);
    expect(h.host.textContent).not.toContain(HOST_TEXT);
    expect(notices[0]?.dataset["level"]).toBe("warning");
  });
}
