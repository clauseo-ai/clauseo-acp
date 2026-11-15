// @vitest-environment happy-dom
// The download the browser saved in Clauseo's own folder for the matter (NoticeKind::DownloadOutsideMatter), over
// FakeTransport: its one quiet line, and Show in Finder after it, which reveals the file by the open matter and its
// name, never by a path. Apart from rows.test.tsx, which is at the size limit.
import { beforeAll, expect, test } from "vitest";
import { MATTER, reset, userMessage, uuid } from "../../testing/stream-script.ts";
import type { ViewEntry } from "../../transport/protocol.ts";
import { loadThread } from "./chunk.ts";
import { mountRoom } from "./harness.ts";

// the thread's chunk loads once, before the first mount, which would otherwise spend its time on the import
beforeAll(async () => void (await loadThread()), 60_000);

const download = (name: unknown): ViewEntry => ({
  notice: {
    order: 6,
    event: uuid(1, 10),
    level: "info",
    text: "The host's own text.",
    kind: { download_outside_matter: { name } } as never,
    ts: 6000,
  },
});

test("the line ends with Show in Finder, which reveals the file in the open matter's folder for downloads", async () => {
  const h = await mountRoom({ platform: "mac" });
  try {
    h.deliver([reset([userMessage(5, 1, "Q"), download("Judgment 12-03-2024.pdf")])]);
    const row = h.one("notice");
    expect(row?.textContent).toContain("The browser saved");
    expect(row?.textContent).not.toContain("The host's own text.");
    const button = h.one("notice-reveal");
    expect(button?.textContent).toBe("Show in Finder");
    await h.click(button);
    await h.settle();
    expect(h.transport.calls.filter((c) => c.call === "reveal")).toEqual([
      { call: "reveal", target: { download: { matter: MATTER, name: "Judgment 12-03-2024.pdf" } } },
    ]);
  } finally {
    await h.unmount();
    document.body.replaceChildren();
  }
});

test("a malformed download notice shows the host's text and no button", async () => {
  const h = await mountRoom();
  try {
    h.deliver([reset([userMessage(5, 1, "Q"), download("")])]);
    expect(h.one("notice")?.textContent).toBe("The host's own text.");
    expect(h.one("notice-reveal")).toBeNull();
  } finally {
    await h.unmount();
    document.body.replaceChildren();
  }
});
