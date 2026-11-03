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
    "Off. This Claude login can't search the web. Clauseo can still read web pages.",
  ],
  [browser("available"), "On. Clauseo can open web pages in its own browser."],
  [browser(off("off")), "Off. You turned it off in Settings."],
  [browser(off("locked_off")), "Off. Managed by your organisation."],
  [
    browser(off("managed_by_firm")),
    "Off. Your organisation chooses the tools Claude has on this computer, so Clauseo can't add its browser.",
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

const settingsButtons = () => [
  ...document.body.querySelectorAll<HTMLElement>('[data-testid="tool-settings"]'),
];
const foot = () => document.body.querySelector<HTMLElement>('[data-testid="tool-settings-foot"]');

test("both tools off: one Open Settings at the popover's foot, under what a change reaches, and it opens Settings", async () => {
  h = await open([web(off("off")), browser(off("off"))]);
  await openPopover();
  expect(settingsButtons()).toHaveLength(1);
  expect(row("web_search")?.querySelector("button")).toBeNull();
  expect(row("browser")?.querySelector("button")).toBeNull();
  expect(foot()?.querySelector("p")?.textContent).toBe("Changes apply to new conversations.");
  expect(popup()?.lastElementChild).toBe(foot());
  // parted from the rows by a hairline, outside every row, so it reads as the whole popover's
  expect(foot()?.classList.contains("tool-pop-foot")).toBe(true);
  expect(foot()?.closest('[data-testid="tool-row"]')).toBeNull();
  expect(settingsButtons()[0]?.textContent).toBe("Open Settings");
  await h.click(settingsButtons()[0]);
  expect(h.transport.only("open_settings")).toHaveLength(1);
});

test("one tool off: the same foot, once", async () => {
  h = await open([web(off("off"))]);
  await openPopover();
  expect(settingsButtons()).toHaveLength(1);
  expect(foot()?.textContent).toContain("Changes apply to new conversations.");
});

test("Off without the settings window offers no Open Settings and no foot; locked off never has a button", async () => {
  h = await open([web(off("off")), browser(off("locked_off"))], false);
  await openPopover();
  expect(settingsButtons()).toEqual([]);
  expect(foot()).toBeNull();
  expect(document.body.textContent).not.toContain("Changes apply to new conversations.");
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
    "Clauseo can download its own Chrome from Google, about 200\u00a0MB, once for all conversations. It needs about 1\u00a0GB free.",
  ]);
  const download = inRow("browser", "tool-download");
  expect(download?.textContent).toBe("Download Chrome");
  await h.click(download);
  // a second press can't happen: the button is gone
  expect(inRow("browser", "tool-download")).toBeNull();
  download?.click();
  expect(h.transport.only("install_browser")).toHaveLength(1);
  expect(words("browser")).toEqual(["Off until Chrome is downloaded."]);
  expect(row("browser")?.textContent).toContain("Downloading Chrome. 0 of 201\u00a0MB.");
  await play({ downloading: { bytes: 64_400_000, total: 201_000_000 } });
  const bar = row("browser")?.querySelector('[role="progressbar"]');
  expect(bar?.getAttribute("aria-valuetext")).toBe("Downloading Chrome. 64 of 201\u00a0MB.");
  expect(row("browser")?.textContent).toContain("Downloading Chrome. 64 of 201\u00a0MB.");
  expect(row("browser")?.querySelector("button")).toBeNull();
  await play("unpacking");
  expect(words("browser")).toEqual(["Off until Chrome is downloaded."]);
  const full = row("browser")?.querySelector('[role="progressbar"]');
  expect(full?.getAttribute("aria-valuetext")).toBe("Unpacking Chrome.");
  expect(full?.getAttribute("aria-valuenow")).toBe(full?.getAttribute("aria-valuemax"));
  await play("done");
  expect(words("browser")).toEqual(["Chrome is ready. New conversations get the browser."]);
  expect(row("browser")?.querySelector('[role="progressbar"]')).toBeNull();
  // the phrase on the status line never changes for the download
  expect(line()?.textContent).toBe("Browser off");
});

/** Enter on the focused element, as a browser takes it: keydown, and on a button its click. */
const pressEnter = async () => {
  const at = document.activeElement;
  await act(async () => {
    at?.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }),
    );
    if (at instanceof HTMLButtonElement) at.click();
    at?.dispatchEvent(new KeyboardEvent("keyup", { key: "Enter", bubbles: true }));
  });
  await h!.settle();
};
const popup = () => document.body.querySelector<HTMLElement>('[data-testid="tool-popover"]');

test("opened by a press, the popup itself takes the focus, named by the tool line's words, its button unfocused", async () => {
  h = await open([web(off("off"))]);
  await openPopover();
  await h.settle();
  expect(document.activeElement).toBe(popup());
  expect(document.activeElement?.tagName).not.toBe("BUTTON");
  expect(popup()?.getAttribute("tabindex")).toBe("-1");
  expect(popup()?.getAttribute("aria-labelledby")).toBe(line()?.id);
  expect(settingsButtons()).toHaveLength(1);
});

test("opened by a key, the popup takes the focus too, and an Enter right after opening starts no download", async () => {
  h = await open([browser(off("browser_not_installed"))]);
  await play({ offer: "download", bytes: 201_000_000 });
  line()!.focus();
  await act(async () => {
    line()!.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }),
    );
    line()!.click();
  });
  await act(() => settled());
  await h.settle();
  expect(document.activeElement).toBe(popup());
  expect(inRow("browser", "tool-download")).not.toBeNull();
  await pressEnter();
  expect(h.transport.only("install_browser")).toEqual([]);
  expect(inRow("browser", "tool-download")).not.toBeNull();
});

const MESSAGE = "curl: (56) Recv failure: Connection reset by peer";

test.each([
  ["network", "The connection dropped before Chrome finished downloading."],
  ["size_mismatch", "The download didn't match the file Google published, so Clauseo deleted it."],
  ["signature", "The download didn't match the file Google published, so Clauseo deleted it."],
  ["disk_space", "Chrome couldn't be downloaded. It needs about 1\u00a0GB free on this computer."],
  ["host_refused", "Your network is blocking the download."],
  ["unpack", "Chrome downloaded but couldn't be unpacked."],
  ["cancelled", "The download stopped before it finished."],
  ["a_newer_code", "Chrome couldn't be downloaded."],
] as const)(
  "a failed download (%s) says why and offers Try again, never the host's message",
  async (code, said) => {
    h = await open([browser(off("browser_not_installed"))]);
    await play({ offer: "download", bytes: 200_000_000 });
    await play({ failed: { code, message: MESSAGE } });
    await openPopover();
    expect(words("browser")).toEqual(["Off. There's no Chrome on this computer.", said]);
    expect(document.body.textContent).not.toContain(MESSAGE);
    expect(document.body.innerHTML).not.toContain("Recv failure");
    const retry = inRow("browser", "tool-retry");
    expect(retry?.textContent).toBe("Try again");
    await h.click(retry);
    expect(h.transport.only("install_browser")).toHaveLength(1);
    expect(inRow("browser", "tool-retry")).toBeNull();
  },
);

test("a computer that can't download Chrome says to install it, with no Try again", async () => {
  h = await open([browser(off("browser_not_installed"))]);
  await play({ offer: "download", bytes: 200_000_000 });
  await play({ failed: { code: "unsupported", message: MESSAGE } });
  await openPopover();
  expect(words("browser")).toEqual([
    "Off. There's no Chrome on this computer.",
    "Clauseo can't download Chrome itself. Install Google Chrome to use the browser.",
  ]);
  expect(row("browser")?.querySelector("button")).toBeNull();
  expect(document.body.textContent).not.toContain(MESSAGE);
});

test("a refused InstallBrowser shows no failure: the offer stands again as it stood before the press", async () => {
  h = await open([browser(off("browser_not_installed"))]);
  await play({ offer: "download", bytes: 201_000_000 });
  h.transport.installBrowser = () => Promise.reject(new Error("a download is already running"));
  await openPopover();
  await h.click(inRow("browser", "tool-download"));
  await h.settle();
  expect(words("browser")).toEqual([
    "Off. There's no Chrome on this computer.",
    "Clauseo can download its own Chrome from Google, about 200\u00a0MB, once for all conversations. It needs about 1\u00a0GB free.",
  ]);
  expect(inRow("browser", "tool-download")).not.toBeNull();
  expect(inRow("browser", "tool-retry")).toBeNull();
  expect(row("browser")?.textContent).not.toContain("couldn't");
});

test("the rendered download lines hold every number to its unit with a non-breaking space", async () => {
  h = await open([browser(off("browser_not_installed"))]);
  await play({ offer: "download", bytes: 201_000_000 });
  await openPopover();
  const said = () => row("browser")?.textContent ?? "";
  const texts = [said()];
  await play({ downloading: { bytes: 64_400_000, total: 201_000_000 } });
  texts.push(said());
  await play({ failed: { code: "disk_space", message: "" } });
  texts.push(said());
  for (const text of texts) {
    expect(text).toMatch(/\d\u00a0(MB|GB)/);
    expect(text).not.toMatch(/\d\s(?<!\u00a0)(MB|GB)/u);
  }
});

test("the popover keeps one width through every download state", async () => {
  h = await open([browser(off("browser_not_installed"))]);
  await play({ offer: "download", bytes: 201_000_000 });
  await openPopover();
  const classes = [popup()?.className];
  await play({ downloading: { bytes: 64_400_000, total: 201_000_000 } });
  classes.push(popup()?.className);
  await play("unpacking");
  classes.push(popup()?.className);
  await play("done");
  classes.push(popup()?.className);
  await play({ failed: { code: "network", message: "" } });
  classes.push(popup()?.className);
  expect(classes[0]?.split(" ")).toContain("w-card-tile");
  expect(new Set(classes).size).toBe(1);
});
