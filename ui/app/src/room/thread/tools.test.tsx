// @vitest-environment happy-dom
// The tool line over FakeTransport: the open thread's header's tools as one phrase on the status line, the popover's
// row for each tool, Open Settings, the tools and reasons this version doesn't know left out, and the Chrome download
// (fake: FakeTransport's `browser_install` app events and its install_browser call), from consent to each end.
import { act } from "react";
import { settled } from "@clauseo/components/lib/late.ts";
import { afterEach, expect, test } from "vitest";
import { header, reset } from "../../testing/stream-script.ts";
import type { ToolState } from "../../transport/protocol.ts";
import type { BrowserInstallReport } from "../browser-install.ts";
import { mountRoom, type Harness } from "./harness.ts";

let h: Harness | null = null;
afterEach(async () => {
  // the popover is portalled to the body: close it with the room
  await h?.unmount();
  h = null;
  document.body.innerHTML = "";
});

const web = (availability: ToolState["availability"]): ToolState => ({
  tool: "web_search",
  availability,
});
const browser = (availability: ToolState["availability"]): ToolState => ({
  tool: "browser",
  availability,
});
const off = (reason: string) => ({ unavailable: { reason } }) as ToolState["availability"];

/** The room with Settings and the app feed, the thread's view arriving with these tools on its header. */
async function open(tools: ToolState[] | undefined, settings = true): Promise<Harness> {
  const harness = await mountRoom({
    next: { parts: ["app"], query: () => Promise.reject(new Error("not scripted")) },
    ...(settings ? { openSettings: () => {} } : {}),
  });
  const head = header();
  if (tools !== undefined) head.header!.tools = tools;
  await harness.deliverAwaited([reset([], head)]);
  await act(() => settled());
  return harness;
}

const line = () => h!.one("tool-line");
const status = () => h!.host.querySelector(".room-status-line");
const rows = () => [...document.body.querySelectorAll<HTMLElement>('[data-testid="tool-row"]')];
const row = (tool: string) =>
  document.body.querySelector<HTMLElement>(`[data-testid="tool-row"][data-tool="${tool}"]`);
const words = (tool: string) =>
  [...(row(tool)?.querySelectorAll('[data-testid="tool-row-line"]') ?? [])].map(
    (p) => p.textContent,
  );
const inRow = (tool: string, id: string) =>
  row(tool)?.querySelector<HTMLElement>(`[data-testid="${id}"]`) ?? null;
const play = async (report: BrowserInstallReport) => {
  await act(async () => h!.transport.appEvent({ browser_install: report }));
};
const openPopover = async () => {
  await h!.click(line());
  await act(() => settled());
};

test("a header with tools shows the phrase after the plan, parted by a dot", async () => {
  h = await open([web("available"), browser("available")]);
  expect(line()?.textContent).toBe("Web search and browser on");
  const seg = status()?.querySelector(".room-status-tools");
  expect(seg?.querySelector(".room-dot")).not.toBeNull();
  expect(status()?.lastElementChild).toBe(seg);
});

test("no tools, or an empty list, or a draft with no thread: no segment", async () => {
  h = await open(undefined);
  expect(line()).toBeNull();
  expect(status()?.querySelector(".room-status-tools")).toBeNull();
  await h.unmount();
  h = await open([]);
  expect(line()).toBeNull();
  await h.unmount();
  h = await mountRoom({ threads: [] });
  expect(line()).toBeNull();
});

test("a tool, an availability or a reason this version doesn't know says nothing", async () => {
  h = await open([
    { tool: "unknown", availability: "available" },
    web("sideways"),
    browser(off("unknown")),
  ]);
  expect(line()).toBeNull();
  await h.unmount();
  h = await open([
    { tool: "unknown", availability: "available" },
    browser(off("unknown")),
    web("available"),
  ]);
  expect(line()?.textContent).toBe("Web search on");
  await openPopover();
  expect(rows().map((r) => r.dataset["tool"])).toEqual(["web_search"]);
});

const ROWS: [ToolState, string][] = [
  [web("available"), "On. Clauseo can search the web and read pages."],
  [web(off("off")), "Off. You turned it off in Settings."],
  [web(off("locked_off")), "Off. Managed by your organisation."],
  [
    web(off("no_search_on_this_login")),
    "This Claude login can't search the web. Clauseo can still read web pages.",
  ],
  [browser("available"), "On. Clauseo can open web pages in its own browser."],
  [browser(off("off")), "Off. You turned it off in Settings."],
  [browser(off("locked_off")), "Off. Managed by your organisation."],
  [
    browser(off("managed_by_firm")),
    "Off. Your organisation chooses the tools Claude has on this computer, so Clauseo adds none.",
  ],
  [browser(off("no_proxy")), "Off until Clauseo can check each website the browser opens."],
  [
    browser(off("too_many_browsers")),
    "Off. Other conversations are using every browser this computer can run. One frees up when another conversation closes.",
  ],
  [browser(off("browser_not_installed")), "Off. There's no Chrome on this computer."],
];

test.each(ROWS)("the popover's row for %o says its words", async (tool, said) => {
  h = await open([tool]);
  await openPopover();
  expect(rows()).toHaveLength(1);
  const name = tool.tool === "web_search" ? "Web search" : "Browser";
  expect(row(tool.tool)?.querySelector("p")?.textContent).toBe(name);
  expect(words(tool.tool)).toEqual([said]);
});

test("Off's Open Settings opens the settings window", async () => {
  h = await open([web(off("off")), browser(off("off"))]);
  await openPopover();
  expect(inRow("web_search", "tool-settings")?.textContent).toBe("Open Settings");
  await h.click(inRow("browser", "tool-settings"));
  expect(h.transport.only("open_settings")).toHaveLength(1);
});

test("Off without the settings window offers no Open Settings; locked off never has a button", async () => {
  h = await open([web(off("off")), browser(off("locked_off"))], false);
  await openPopover();
  expect(inRow("web_search", "tool-settings")).toBeNull();
  expect(row("browser")?.querySelector("button")).toBeNull();
});

test("no Chrome and no install state from the host: no download words", async () => {
  h = await open([browser(off("browser_not_installed"))]);
  await openPopover();
  expect(words("browser")).toEqual(["Off. There's no Chrome on this computer."]);
  expect(row("browser")?.querySelector("button")).toBeNull();
});

test("the host offers no download: install Chrome yourself", async () => {
  h = await open([browser(off("browser_not_installed"))]);
  await play({ offer: "none" });
  await openPopover();
  expect(words("browser")).toEqual([
    "Off. There's no Chrome on this computer.",
    "Install Google Chrome to use the browser.",
  ]);
  expect(row("browser")?.querySelector("button")).toBeNull();
});

test("Download Chrome is the consent: it sends InstallBrowser once and goes, then the progress shows", async () => {
  h = await open([browser(off("browser_not_installed"))]);
  await play({ offer: "download", bytes: 201_000_000 });
  await openPopover();
  expect(words("browser")).toEqual([
    "Off. There's no Chrome on this computer.",
    "Clauseo can download Chrome for its own use: about 200 MB from Google, once for every conversation. It needs about 1 GB free.",
  ]);
  const download = inRow("browser", "tool-download");
  expect(download?.textContent).toBe("Download Chrome");
  await h.click(download);
  // a second press can't happen: the button is gone
  expect(inRow("browser", "tool-download")).toBeNull();
  download?.click();
  expect(h.transport.only("install_browser")).toHaveLength(1);
  expect(row("browser")?.textContent).toContain("Downloading Chrome. 0 of 201 MB.");
  await play({ downloading: { bytes: 64_400_000, total: 201_000_000 } });
  const bar = row("browser")?.querySelector('[role="progressbar"]');
  expect(bar?.getAttribute("aria-valuetext")).toBe("Downloading Chrome. 64 of 201 MB.");
  expect(row("browser")?.textContent).toContain("Downloading Chrome. 64 of 201 MB.");
  expect(row("browser")?.querySelector("button")).toBeNull();
  await play("unpacking");
  const full = row("browser")?.querySelector('[role="progressbar"]');
  expect(full?.getAttribute("aria-valuetext")).toBe("Unpacking Chrome.");
  expect(full?.getAttribute("aria-valuenow")).toBe(full?.getAttribute("aria-valuemax"));
  await play("done");
  expect(words("browser")).toEqual([
    "Off. There's no Chrome on this computer.",
    "Chrome is ready. New conversations get the browser.",
  ]);
  expect(row("browser")?.querySelector('[role="progressbar"]')).toBeNull();
  // the phrase on the status line never changes for the download
  expect(line()?.textContent).toBe("Browser off");
});

test.each([
  ["disk", "Chrome couldn't be downloaded. It needs about 1 GB free on this computer."],
  ["refused", "Your network is blocking the download."],
  ["check", "The download didn't match Google's, so Clauseo deleted it."],
  ["other", "Chrome couldn't be downloaded."],
] as const)("a failed download (%s) says why and offers Try again", async (code, said) => {
  h = await open([browser(off("browser_not_installed"))]);
  await play({ offer: "download", bytes: 200_000_000 });
  await play({ failed: { code } });
  await openPopover();
  expect(words("browser")).toEqual(["Off. There's no Chrome on this computer.", said]);
  const retry = inRow("browser", "tool-retry");
  expect(retry?.textContent).toBe("Try again");
  await h.click(retry);
  expect(h.transport.only("install_browser")).toHaveLength(1);
  expect(inRow("browser", "tool-retry")).toBeNull();
});
