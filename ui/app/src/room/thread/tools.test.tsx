// @vitest-environment happy-dom
// The tool line over FakeTransport: the open thread's header's tools as glyphs on the status line, named in words, the
// popover's row for each tool (glyph, name, On or Off, the reason only when off), Open Settings at the foot, the
// tools and reasons this version doesn't know left out, and the Chrome download (FakeTransport's `browser_install`
// app events, and the install_browser commands it is dispatched), from consent to each end.
import { act } from "react";
import { settled } from "@clauseo/components/lib/late.ts";
import { afterEach, expect, test } from "vitest";
import { header, reset } from "../../testing/stream-script.ts";
import type { BrowserInstallState, ToolState } from "../../transport/protocol.ts";
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
const glyphs = () =>
  [...(status()?.querySelectorAll<HTMLElement>(".room-status-tools [data-tool]") ?? [])].map(
    (g) => [g.dataset["tool"], g.dataset["state"]],
  );
const rows = () => [...document.body.querySelectorAll<HTMLElement>('[data-testid="tool-row"]')];
const row = (tool: string) =>
  document.body.querySelector<HTMLElement>(`[data-testid="tool-row"][data-tool="${tool}"]`);
const inRow = (tool: string, id: string) =>
  row(tool)?.querySelector<HTMLElement>(`[data-testid="${id}"]`) ?? null;
const said = (tool: string, id: string) => inRow(tool, id)?.textContent ?? null;
const play = async (state: BrowserInstallState) => {
  await act(async () => h!.transport.appEvent({ browser_install: { state } }));
};
/** The install_browser commands dispatched so far. */
const installs = () =>
  h!.transport.only("dispatch").filter((c) => "install_browser" in c.command.command);
const openPopover = async () => {
  await h!.click(line());
  await act(() => settled());
};
const described = (el: HTMLElement | null) =>
  document.getElementById(el?.getAttribute("aria-describedby") ?? "")?.textContent ?? null;

test("a header with tools shows their glyphs after the plan, parted by a dot, named in words and no visible words", async () => {
  h = await open([web("available"), browser(off("browser_not_installed"))]);
  expect(line()?.getAttribute("aria-label")).toBe("Web search: on. Browser: off, needs Chrome.");
  expect(line()?.textContent).toBe("");
  expect(line()?.querySelectorAll("[data-tool] > svg")).toHaveLength(2);
  for (const svg of line()?.querySelectorAll("[data-tool] > svg") ?? [])
    expect(svg.getAttribute("width")).toBe("14");
  expect(glyphs()).toEqual([
    ["web_search", "on"],
    ["browser", "off"],
  ]);
  const seg = status()?.querySelector(".room-status-tools");
  expect(seg?.querySelector(".room-dot")).not.toBeNull();
  expect(status()?.lastElementChild).toBe(seg);
});

test.each([
  [
    [web("available"), browser("available")],
    [
      ["web_search", "on"],
      ["browser", "on"],
    ],
    "Web search: on. Browser: on.",
  ],
  [[web(off("off"))], [["web_search", "off"]], "Web search: off, in Settings."],
  [
    [web(off("no_search_on_this_login"))],
    [["web_search", "off"]],
    "Web search: off, can't search on this login.",
  ],
  [
    [web(off("locked_off")), browser(off("managed_by_firm"))],
    [
      ["web_search", "locked"],
      ["browser", "locked"],
    ],
    "Web search: off, managed by firm. Browser: off, managed by firm.",
  ],
  [[browser(off("locked_off"))], [["browser", "locked"]], "Browser: off, managed by firm."],
  [[browser(off("no_proxy"))], [["browser", "off"]], "Browser: off, not available yet."],
  [
    [browser(off("too_many_browsers"))],
    [["browser", "off"]],
    "Browser: off, too many browsers open.",
  ],
] as [ToolState[], string[][], string][])(
  "each state on its glyph, and in the line's name: %o",
  async (tools, states, name) => {
    h = await open(tools);
    expect(glyphs()).toEqual(states);
    expect(line()?.getAttribute("aria-label")).toBe(name);
    // a slashed glyph for off, the lock for the firm's, the glyph as drawn for on
    const svgs = [...(line()?.querySelectorAll("[data-tool] > svg") ?? [])];
    states.forEach(([, state], i) => {
      expect(svgs[i]?.querySelector('[data-part="slash"]') !== null).toBe(state === "off");
      expect(svgs[i]?.querySelector('[data-part="lock"]') !== null).toBe(state === "locked");
    });
  },
);

test("focused from the keyboard, the tool line's tooltip says the same words as its name", async () => {
  h = await open([web("available"), browser(off("browser_not_installed"))]);
  await act(async () => {
    line()!.dispatchEvent(new KeyboardEvent("keydown", { key: "Tab", bubbles: true }));
    line()!.focus();
  });
  await h.settle();
  const tips = [...document.body.querySelectorAll<HTMLElement>("[data-open]")].filter(
    (el) => el.textContent === "Web search: on. Browser: off, needs Chrome.",
  );
  expect(tips.length).toBeGreaterThan(0);
  // raised over the thread and the popover's plane: the shared tooltip stacks in the page's layer, under both
  const positioner =
    tips.find((el) => el.getAttribute("role") === "presentation") ?? tips[0]?.parentElement;
  expect(positioner?.classList.contains("z-30")).toBe(true);
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

test("a tool, an availability or a reason this version doesn't know shows no glyph", async () => {
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
  expect(glyphs()).toEqual([["web_search", "on"]]);
  expect(line()?.getAttribute("aria-label")).toBe("Web search: on.");
  await openPopover();
  expect(rows().map((r) => r.dataset["tool"])).toEqual(["web_search"]);
});

const ROWS: [ToolState, string | null][] = [
  [web("available"), null],
  [web(off("off")), "In Settings"],
  [web(off("locked_off")), "Managed by firm"],
  [web(off("no_search_on_this_login")), "Can't search on this login"],
  [browser("available"), null],
  [browser(off("off")), "In Settings"],
  [browser(off("locked_off")), "Managed by firm"],
  [browser(off("managed_by_firm")), "Managed by firm"],
  [browser(off("no_proxy")), "Not available yet"],
  [browser(off("too_many_browsers")), "Too many browsers open"],
  [browser(off("browser_not_installed")), "Needs Chrome"],
];

test.each(ROWS)(
  "the popover's row for %o: glyph, name, chip, and the reason only when off",
  async (tool, reason) => {
    h = await open([tool]);
    await openPopover();
    expect(rows()).toHaveLength(1);
    const r = row(tool.tool)!;
    const svg = r.firstElementChild;
    expect(svg?.tagName.toLowerCase()).toBe("svg");
    expect(svg?.getAttribute("width")).toBe("16");
    expect(said(tool.tool, "tool-row-name")).toBe(
      tool.tool === "web_search" ? "Web search" : "Browser",
    );
    expect(said(tool.tool, "tool-row-chip")).toBe(reason === null ? "On" : "Off");
    expect(said(tool.tool, "tool-row-reason")).toBe(reason);
    // In Settings sits beside the Off chip, in its row; every other reason on a line of its own
    const reasonEl = inRow(tool.tool, "tool-row-reason");
    if (reasonEl !== null)
      expect(reasonEl.parentElement === inRow(tool.tool, "tool-row-chip")?.parentElement).toBe(
        tool.availability !== "available" && reason === "In Settings",
      );
    // nothing else in words: the name, the chip and the reason; with no Chrome, the offer's words under it (the
    // button, its hidden tooltip and the size)
    const offer =
      reason === "Needs Chrome"
        ? "Download Chrome" + "From Google, once. Needs 1\u00a0GB free." + "200\u00a0MB"
        : "";
    expect(r.textContent).toBe(
      (tool.tool === "web_search" ? "Web search" : "Browser") +
        (reason === null ? "On" : "Off") +
        (reason ?? "") +
        offer,
    );
  },
);

const settingsButtons = () => [
  ...document.body.querySelectorAll<HTMLElement>('[data-testid="tool-settings"]'),
];
const foot = () => document.body.querySelector<HTMLElement>('[data-testid="tool-settings-foot"]');

test("both tools off in Settings: one Open Settings at the popover's foot, its tooltip what a change reaches, and it opens Settings", async () => {
  h = await open([web(off("off")), browser(off("off"))]);
  await openPopover();
  expect(settingsButtons()).toHaveLength(1);
  expect(row("web_search")?.querySelector("button")).toBeNull();
  expect(row("browser")?.querySelector("button")).toBeNull();
  expect(popup()?.lastElementChild).toBe(foot());
  expect(foot()?.classList.contains("tool-pop-foot")).toBe(true);
  expect(foot()?.closest('[data-testid="tool-row"]')).toBeNull();
  // no visible line: the words are the button's tooltip and its description
  expect(foot()?.querySelector("p")).toBeNull();
  expect(described(settingsButtons()[0]!)).toBe("Changes apply to new conversations.");
  expect(settingsButtons()[0]?.textContent).toBe("Open Settings");
  await h.click(settingsButtons()[0]);
  expect(h.transport.only("open_settings")).toHaveLength(1);
});

test("one tool off in Settings: the same foot, once", async () => {
  h = await open([web(off("off")), browser("available")]);
  await openPopover();
  expect(settingsButtons()).toHaveLength(1);
});

test("no tool off in Settings, or no settings window: no Open Settings and no foot", async () => {
  h = await open([web(off("locked_off")), browser(off("no_proxy"))]);
  await openPopover();
  expect(settingsButtons()).toEqual([]);
  expect(foot()).toBeNull();
  await h.unmount();
  document.body.innerHTML = "";
  h = await open([web(off("off")), browser(off("locked_off"))], false);
  await openPopover();
  expect(settingsButtons()).toEqual([]);
  expect(foot()).toBeNull();
  expect(document.body.textContent).not.toContain("Changes apply to new conversations.");
});

test("no Chrome and no download yet: the reason, and the offer to download it", async () => {
  h = await open([browser(off("browser_not_installed"))]);
  await openPopover();
  expect(said("browser", "tool-row-reason")).toBe("Needs Chrome");
  expect(inRow("browser", "tool-download")?.textContent).toBe("Download Chrome");
  expect(installs()).toEqual([]);
});

test("Download Chrome is the consent: it sends install_browser once and goes, then the bar, Unpacking, and Ready for new conversations", async () => {
  h = await open([browser(off("browser_not_installed"))]);
  await openPopover();
  const download = inRow("browser", "tool-download");
  expect(download?.textContent).toBe("Download Chrome");
  expect(download?.querySelector("svg")).not.toBeNull();
  expect(said("browser", "tool-download-size")).toBe("200\u00a0MB");
  expect(described(download)).toBe("From Google, once. Needs 1\u00a0GB free.");
  await h.click(download);
  // a second press can't happen: the button is gone
  expect(inRow("browser", "tool-download")).toBeNull();
  download?.click();
  expect(installs()).toHaveLength(1);
  // the host's first Downloading brings the total: until then the bar has no words
  expect(row("browser")?.querySelector('[role="progressbar"]')).not.toBeNull();
  await play({ downloading: { bytes: 64_400_000, total: 200_000_000 } });
  const bar = row("browser")?.querySelector('[role="progressbar"]');
  expect(bar?.getAttribute("aria-valuetext")).toBe("64 of 200\u00a0MB");
  expect(bar?.getAttribute("aria-label")).toBe("Download Chrome");
  expect(bar?.querySelector("svg")).not.toBeNull();
  expect(said("browser", "tool-progress-text")).toBe("64 of 200\u00a0MB");
  expect(said("browser", "tool-row-reason")).toBe("Needs Chrome");
  expect(row("browser")?.querySelector("button")).toBeNull();
  await play("unpacking");
  // no bar while unpacking (determinate bars only, nothing spins): the download glyph and the word
  expect(row("browser")?.querySelector('[role="progressbar"]')).toBeNull();
  expect(said("browser", "tool-progress-text")).toBe("Unpacking");
  expect(inRow("browser", "tool-progress-text")?.querySelector("svg")).not.toBeNull();
  await play("done");
  // done keeps a reason, with a tick and a tooltip on when the browser comes
  expect(said("browser", "tool-row-reason")).toBe("Ready for new conversations");
  const ready = inRow("browser", "tool-row-reason")!;
  expect(ready.querySelector("svg")).not.toBeNull();
  expect(ready.getAttribute("tabindex")).toBe("0");
  expect(described(ready)).toBe("The browser comes with your next conversation.");
  expect(inRow("browser", "tool-row-note")).toBeNull();
  expect(row("browser")?.querySelector('[role="progressbar"]')).toBeNull();
  // the line never changes for the download: the header still says no Chrome
  expect(line()?.getAttribute("aria-label")).toBe("Browser: off, needs Chrome.");
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

test("opened by a press, the popup itself takes the focus, named by the tool line, its button unfocused", async () => {
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
  expect(installs()).toEqual([]);
  expect(inRow("browser", "tool-download")).not.toBeNull();
});

const MESSAGE = "curl: (56) Recv failure: Connection reset by peer";

test.each([
  ["network", "Connection dropped"],
  ["size_mismatch", "File didn't match"],
  ["signature", "File didn't match"],
  ["disk_space", "Needs 1\u00a0GB free"],
  ["host_refused", "Network blocked it"],
  ["unpack", "Couldn't unpack"],
  ["a_newer_code", "Download failed"],
] as const)(
  "a failed download (%s) says its cause by the break glyph, with a Try again icon button, never the host's message",
  async (code, cause) => {
    h = await open([browser(off("browser_not_installed"))]);
    await play({ failed: { code: code as never, message: MESSAGE } });
    await openPopover();
    expect(said("browser", "tool-row-reason")).toBe("Needs Chrome");
    expect(said("browser", "tool-row-note")).toBe(cause);
    expect(inRow("browser", "tool-row-note")?.parentElement?.querySelector("svg")).not.toBeNull();
    expect(document.body.textContent).not.toContain(MESSAGE);
    expect(document.body.innerHTML).not.toContain("Recv failure");
    const retry = inRow("browser", "tool-retry");
    expect(retry?.getAttribute("aria-label")).toBe("Try again");
    expect(retry?.textContent).toBe("");
    expect(retry?.querySelector("svg")).not.toBeNull();
    await h.click(retry);
    expect(installs()).toHaveLength(1);
    expect(inRow("browser", "tool-retry")).toBeNull();
  },
);

test("a computer that can't download Chrome says to install it, with no Try again", async () => {
  h = await open([browser(off("browser_not_installed"))]);
  await play({ failed: { code: "unsupported", message: MESSAGE } });
  await openPopover();
  expect(said("browser", "tool-row-reason")).toBe("Needs Chrome");
  expect(said("browser", "tool-row-note")).toBe("Install Chrome to use it");
  expect(row("browser")?.querySelector("button")).toBeNull();
  expect(document.body.textContent).not.toContain(MESSAGE);
});

test("a cancelled download shows the offer again, with no line and no Try again", async () => {
  h = await open([browser(off("browser_not_installed"))]);
  await play({ downloading: { bytes: 64_400_000, total: 200_000_000 } });
  await play({ failed: { code: "cancelled", message: MESSAGE } });
  await openPopover();
  expect(inRow("browser", "tool-download")?.textContent).toBe("Download Chrome");
  expect(inRow("browser", "tool-row-note")).toBeNull();
  expect(inRow("browser", "tool-retry")).toBeNull();
});

test("while a download runs the row has no Install to press: the bar shows the host's latest bytes", async () => {
  h = await open([browser(off("browser_not_installed"))]);
  await play({ downloading: { bytes: 64_400_000, total: 200_000_000 } });
  await play({ downloading: { bytes: 80_000_000, total: 200_000_000 } });
  await openPopover();
  expect(inRow("browser", "tool-download")).toBeNull();
  expect(installs()).toEqual([]);
  expect(
    row("browser")?.querySelector('[role="progressbar"]')?.getAttribute("aria-valuetext"),
  ).toBe("80 of 200\u00a0MB");
});

test("a catch-up's latest state shows when the window opens: the bar, not the offer", async () => {
  h = await open([browser(off("browser_not_installed"))]);
  await play("unpacking");
  await openPopover();
  expect(inRow("browser", "tool-download")).toBeNull();
  expect(said("browser", "tool-progress-text")).toBe("Unpacking");
});

test("a refused Install shows no failure: the offer stands again as it stood before the press", async () => {
  h = await open([browser(off("browser_not_installed"))]);
  h.transport.host.dispatch = () => Promise.reject(new Error("the host refused it"));
  await openPopover();
  await h.click(inRow("browser", "tool-download"));
  await h.settle();
  expect(inRow("browser", "tool-download")).not.toBeNull();
  expect(said("browser", "tool-download-size")).toBe("200\u00a0MB");
  expect(inRow("browser", "tool-retry")).toBeNull();
  expect(inRow("browser", "tool-row-note")).toBeNull();
  expect(row("browser")?.querySelector('[role="progressbar"]')).toBeNull();
});

test("the rendered download words hold every number to its unit with a non-breaking space", async () => {
  h = await open([browser(off("browser_not_installed"))]);
  await openPopover();
  const all = () => row("browser")?.textContent ?? "";
  const texts = [all()];
  await play({ downloading: { bytes: 64_400_000, total: 200_000_000 } });
  texts.push(all());
  await play({ failed: { code: "disk_space", message: "" } });
  texts.push(all());
  for (const text of texts) {
    expect(text).toMatch(/\d\u00a0(MB|GB)/);
    expect(text).not.toMatch(/\d\s(?<!\u00a0)(MB|GB)/u);
  }
});

test("the popover keeps one width through every download state", async () => {
  h = await open([browser(off("browser_not_installed"))]);
  await openPopover();
  const classes = [popup()?.className];
  await play({ downloading: { bytes: 64_400_000, total: 200_000_000 } });
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
