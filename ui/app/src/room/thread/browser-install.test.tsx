// @vitest-environment happy-dom
// Clauseo's own browser in the tool line's popover over FakeTransport: under the browser with none, on a Mac (and the
// fake host's Linux), Get a browser for Clauseo sends install_browser; the host's `browser_install` app events (and its
// catch-up) then show how far the download has got, with Cancel sending cancel_browser_install, the wait for the
// network, the update when the copy is behind, the firm's policy and done. The failures with words of their own (TLS, a
// refused address, the server's error, a full disk, no network after the retries) are the offer with its line above
// it; any other failure is the offer again. Never on Windows.
import { act } from "react";
import { settled } from "@clauseo/components/lib/late.ts";
import { afterEach, expect, test } from "vitest";
import { header, reset } from "../../testing/stream-script.ts";
import type { BrowserInstallState, ToolState } from "../../transport/protocol.ts";
import { mountRoom, type Harness } from "./harness.ts";

let h: Harness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.innerHTML = "";
});

const MB = 1_000_000;
const TLS =
  "Clauseo couldn't connect securely to get the browser. Ask your firm's IT team for help.";
const HOST_REFUSED =
  "The download address was refused, so Clauseo couldn't get the browser. Try again later.";
const SERVER_REFUSED =
  "Google's download server answered with an error, so Clauseo couldn't get the browser. Try again later.";
const DISK_SPACE =
  "There isn't enough free space on this computer to get the browser. Free up about 1\u00a0GB and try again.";
const NETWORK =
  "Clauseo couldn't reach the internet to get the browser. Check your connection and try again.";
const MISSING: ToolState = {
  tool: "browser",
  availability: { unavailable: { reason: "browser_not_installed" } } as ToolState["availability"],
};

/** The room with the app feed, its thread's header saying no browser is on the computer; `first` is the app's
 * catch-up. */
async function open(
  platform?: "mac" | "windows",
  first: readonly BrowserInstallState[] = [],
): Promise<Harness> {
  const harness = await mountRoom({
    next: {
      parts: ["app"],
      query: () => Promise.reject(new Error("not scripted")),
      first: () => first.map((state) => ({ browser_install: { state } })),
    },
    ...(platform === undefined ? {} : { platform }),
  });
  const head = header();
  head.header!.tools = [MISSING];
  await harness.deliverAwaited([reset([], head)]);
  await act(() => settled());
  await harness.click(harness.one("tool-line"));
  await act(() => settled());
  return harness;
}

const inRow = (id: string) =>
  document.body.querySelector<HTMLElement>(
    `[data-testid="tool-row"][data-tool="browser"] [data-testid="${id}"]`,
  );
const said = (id: string) => inRow(id)?.textContent ?? null;
const play = async (state: BrowserInstallState) => {
  await act(async () => h!.transport.appEvent({ browser_install: { state } }));
};
const sent = () =>
  h!.transport
    .only("dispatch")
    .map((c) => Object.keys(c.command.command)[0])
    .filter((k) => k === "install_browser" || k === "cancel_browser_install");

test.each([undefined, "mac"] as const)(
  "(%s) the offer sends install_browser once, waiting; the download shows its size, percent, bar and Cancel",
  async (platform) => {
    h = await open(platform);
    const offer = inRow("tool-install");
    expect(offer?.textContent).toBe("Get a browser for Clauseo");
    expect(said("tool-row-full")).toBe(
      "Clauseo didn't find Google Chrome, Microsoft Edge, Brave or Chromium on this computer. Install one, then start a new conversation.",
    );
    await h.click(offer);
    expect(sent()).toEqual(["install_browser"]);
    expect(inRow("tool-install")?.getAttribute("aria-busy")).toBe("true");
    await h.click(inRow("tool-install"));
    expect(sent()).toEqual(["install_browser"]);

    await play({ downloading: { bytes: 84 * MB, total: 162 * MB, behind: false } });
    expect(inRow("tool-install")).toBeNull();
    expect(inRow("tool-row-full")).toBeNull();
    expect(said("tool-install-line")).toBe(
      "Getting Clauseo's browser: 84\u00a0MB of 162\u00a0MB (52%)",
    );
    expect(inRow("tool-install-behind")).toBeNull();
    const bar = inRow("tool-progress");
    expect(bar?.getAttribute("aria-valuenow")).toBe(String(84 * MB));
    expect(said("tool-progress-text")).toBe("52%");

    expect(said("tool-install-cancel")).toBe("Cancel");
    await h.click(inRow("tool-install-cancel"));
    expect(sent()).toEqual(["install_browser", "cancel_browser_install"]);
    // the host's answer: the offer again, with no words of its own
    await play({ failed: { code: "cancelled", message: "stopped" } });
    expect(said("tool-install")).toBe("Get a browser for Clauseo");
    expect(inRow("tool-install-line")).toBeNull();
  },
);

test("unpacking shows the same line at 100%", async () => {
  h = await open("mac");
  await play({ downloading: { bytes: 160 * MB, total: 162 * MB, behind: false } });
  expect(said("tool-install-line")).toBe(
    "Getting Clauseo's browser: 160\u00a0MB of 162\u00a0MB (99%)",
  );
  await play("unpacking");
  expect(said("tool-install-line")).toBe(
    "Getting Clauseo's browser: 162\u00a0MB of 162\u00a0MB (100%)",
  );
  expect(said("tool-install-cancel")).toBe("Cancel");
});

test("behind on security updates: the update's words over the progress", async () => {
  h = await open("mac", [{ downloading: { bytes: 81 * MB, total: 162 * MB, behind: true } }]);
  expect(said("tool-install-behind")).toBe(
    "Clauseo's browser is behind on security updates. Updating it now.",
  );
  expect(said("tool-install-line")).toBe(
    "Getting Clauseo's browser: 81\u00a0MB of 162\u00a0MB (50%)",
  );
  expect(said("tool-install-cancel")).toBe("Cancel");
});

test("offline: the wait, and Cancel sends cancel_browser_install", async () => {
  h = await open("mac", ["offline"]);
  expect(said("tool-install-line")).toBe("Waiting for an internet connection to get the browser.");
  expect(inRow("tool-install")).toBeNull();
  await h.click(inRow("tool-install-cancel"));
  expect(sent()).toEqual(["cancel_browser_install"]);
});

test("the firm forbids it: its line, and nothing to press", async () => {
  h = await open("mac", [{ failed: { code: "firm_forbids", message: "policy" } }]);
  expect(said("tool-install-line")).toBe("Your firm doesn't allow Clauseo to download a browser.");
  expect(inRow("tool-install")).toBeNull();
  expect(inRow("tool-install-cancel")).toBeNull();
});

test("done: ready, and a new conversation uses it", async () => {
  h = await open("mac");
  await play("done");
  expect(said("tool-row-reason")).toBe("Ready for new conversations");
  expect(said("tool-install-line")).toBe(
    "Clauseo's browser is ready. Start a new conversation to use it.",
  );
  expect(inRow("tool-row-full")).toBeNull();
  expect(inRow("tool-install")).toBeNull();
});

test("a refused address and an HTTP error show different lines, each with the offer under it", async () => {
  h = await open("mac", [{ failed: { code: "host_refused", message: "log only" } }]);
  expect(said("tool-install-line")).toBe(HOST_REFUSED);
  expect(said("tool-install")).toBe("Get a browser for Clauseo");
  // the install-Chrome note stays, as for the plain offer
  expect(said("tool-row-full")).toBe(
    "Clauseo didn't find Google Chrome, Microsoft Edge, Brave or Chromium on this computer. Install one, then start a new conversation.",
  );
  await play({ failed: { code: "server_refused", message: "log only" } });
  expect(said("tool-install-line")).toBe(SERVER_REFUSED);
  expect(said("tool-install")).toBe("Get a browser for Clauseo");
  expect(SERVER_REFUSED).not.toBe(HOST_REFUSED);
  // and the offer under it starts a download
  await h.click(inRow("tool-install"));
  expect(sent()).toEqual(["install_browser"]);
  await play({ downloading: { bytes: 1 * MB, total: 2 * MB, behind: false } });
  expect(inRow("tool-install-line")?.textContent).toBe(
    "Getting Clauseo's browser: 1\u00a0MB of 2\u00a0MB (50%)",
  );
  expect(inRow("tool-install")).toBeNull();
});

test.each([
  ["tls", { failed: { code: "tls", message: "log only" } }, TLS],
  ["server_refused", { failed: { code: "server_refused", message: "log only" } }, SERVER_REFUSED],
  ["host_refused", { failed: { code: "host_refused", message: "log only" } }, HOST_REFUSED],
  ["disk_space", { failed: { code: "disk_space", message: "log only" } }, DISK_SPACE],
  ["network", { failed: { code: "network", message: "log only" } }, NETWORK],
] as const)(
  "a %s failure is the offer with its line above it, and the offer starts another",
  async (_code, state, line) => {
    h = await open("mac");
    await play(state);
    expect(said("tool-install-line")).toBe(line);
    expect(said("tool-install")).toBe("Get a browser for Clauseo");
    expect(document.body.textContent).not.toContain("log only");
    await h.click(inRow("tool-install"));
    expect(sent()).toEqual(["install_browser"]);
  },
);

test("a line of the offer in Hindi", async () => {
  h = await mountRoom({
    display: { language: "hi" },
    next: {
      parts: ["app"],
      query: () => Promise.reject(new Error("not scripted")),
      first: () => [{ browser_install: { state: { failed: { code: "tls", message: "" } } } }],
    },
  });
  const head = header();
  head.header!.tools = [MISSING];
  await h.deliverAwaited([reset([], head)]);
  await act(() => settled());
  await h.click(h.one("tool-line"));
  await act(() => settled());
  expect(said("tool-install-line")).toBe(
    "Clauseo ब्राउज़र डाउनलोड करने के लिए सुरक्षित रूप से कनेक्ट नहीं कर सका। सहायता के लिए अपनी फ़र्म की IT टीम से पूछें।",
  );
  expect(said("tool-install")).toBe("Clauseo के लिए ब्राउज़र डाउनलोड करें");
});

test.each(["signature", "unpack", "unknown", "size_mismatch", "cancelled"] as const)(
  "a %s failure is the offer again, with no line, and it starts another",
  async (code) => {
    h = await open("mac", [{ failed: { code, message: "log only" } }]);
    expect(said("tool-install")).toBe("Get a browser for Clauseo");
    expect(inRow("tool-install-line")).toBeNull();
    expect(document.body.textContent).not.toContain("log only");
    await h.click(inRow("tool-install"));
    expect(sent()).toEqual(["install_browser"]);
  },
);

test("on Windows: nothing of Clauseo's own browser, whatever the host says", async () => {
  h = await open("windows", [{ downloading: { bytes: 1, total: 2, behind: true } }]);
  expect(inRow("tool-install")).toBeNull();
  expect(inRow("tool-install-getting")).toBeNull();
  for (const state of [
    "offline",
    "done",
    { failed: { code: "firm_forbids", message: "" } },
  ] as const) {
    await play(state);
    expect(inRow("tool-install-line")).toBeNull();
  }
  expect(said("tool-row-full")).toBe(
    "Install Google Chrome or Microsoft Edge, then start a new conversation.",
  );
  expect(sent()).toEqual([]);
});
