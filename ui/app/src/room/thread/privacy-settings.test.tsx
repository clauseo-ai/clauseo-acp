// @vitest-environment happy-dom
// Open Privacy settings under a failed turn's stop (privacy-settings.tsx): on the Mac for matter.folder_permission only,
// calling the desktop's open_privacy_settings; never for another code, and never on Windows or Linux.
import { errorSentence, errorWords, PRIVACY_KEYS } from "@clauseo/components/lib/words-errors.ts";
import { afterEach, expect, test } from "vitest";
import { agentText, reset, step, userMessage } from "../../testing/stream-script.ts";
import type { Platform } from "@clauseo/components/lib/display.ts";
import { internalError } from "../../transport/host-transport.ts";
import type { ErrorCode } from "../../transport/protocol.ts";
import { mountRoom, type Harness } from "./harness.ts";

let h: Harness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
});

/** The room on `platform`, over a thread whose last turn failed with `code`. */
async function failedWith(
  code: ErrorCode,
  platform: Platform,
  openPrivacySettings?: () => void,
): Promise<Harness> {
  const room = await mountRoom({
    platform,
    ...(openPrivacySettings === undefined ? {} : { openPrivacySettings }),
  });
  room.deliver([
    reset([
      userMessage(5, 1, "Summarise the lease"),
      agentText(6, 1, "The lease"),
      step(7, { stop: "failed", failure: code }),
    ]),
  ]);
  return room;
}

test("on the Mac, matter.folder_permission's stop offers Open Privacy settings with its line, and it opens the pane", async () => {
  h = await failedWith("matter.folder_permission", "mac");
  expect(h.one("turn-stopped")?.querySelector("[data-code]")?.textContent).toBe(
    errorWords("en", "matter.folder_permission"),
  );
  const button = h.one("open-privacy-settings");
  expect(button?.textContent).toBe("Open Privacy settings");
  expect(button?.textContent).toBe(errorSentence("en", PRIVACY_KEYS.button));
  expect(h.one("privacy-settings-line")?.textContent).toBe(
    "Turn on access for Clauseo, then ask again.",
  );
  expect(h.transport.only("open_privacy_settings")).toEqual([]);
  await h.click(button);
  expect(h.transport.only("open_privacy_settings")).toEqual([{ call: "open_privacy_settings" }]);
});

test("a refused open leaves the stop as it was", async () => {
  h = await failedWith("matter.folder_permission", "mac", () => {
    throw internalError("refused");
  });
  await h.click(h.one("open-privacy-settings"));
  expect(h.transport.only("open_privacy_settings")).toHaveLength(1);
  expect(h.one("open-privacy-settings")).not.toBeNull();
  expect(h.one("privacy-settings-line")).not.toBeNull();
});

test("Windows and Linux never show it, and the stop's words still stand", async () => {
  for (const platform of ["windows", "linux"] as const) {
    h = await failedWith("matter.folder_permission", platform);
    expect(h.one("turn-stopped")?.querySelector("[data-code]")?.textContent).toBe(
      errorWords("en", "matter.folder_permission"),
    );
    expect(h.one("open-privacy-settings")).toBeNull();
    expect(h.one("privacy-settings-line")).toBeNull();
    await h.unmount();
    h = null;
    document.body.replaceChildren();
  }
});

test("on the Mac, no other code shows it", async () => {
  const others: ErrorCode[] = [
    "matter.folder_missing",
    "matter.folder_not_a_matter",
    "agent.launch_failed",
  ];
  for (const code of others) {
    h = await failedWith(code, "mac");
    expect(h.one("turn-stopped")?.dataset["failure"]).toBe(code);
    expect(h.one("open-privacy-settings")).toBeNull();
    expect(h.one("privacy-settings-line")).toBeNull();
    await h.unmount();
    h = null;
    document.body.replaceChildren();
  }
});
