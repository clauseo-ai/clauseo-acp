// @vitest-environment happy-dom
// The tool line over FakeTransport: the open thread's header's tools as glyphs on the status line, named in words, the
// popover's row for each tool (glyph, name, On or Off, the reason only when off), Open Settings at the foot, the
// tools and reasons this version doesn't know left out, what to install with no browser on the computer, by its OS,
// and the firm's browser policy re-read (FakeTransport's `browser_policy` app events) with its Try again.
import { act } from "react";
import { settled } from "@clauseo/components/lib/late.ts";
import { afterEach, expect, test } from "vitest";
import { header, reset } from "../../testing/stream-script.ts";
import type { BrowserPolicyState, ToolState } from "../../transport/protocol.ts";
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

/** The room with Settings and the app feed, the thread's view arriving with these tools on its header; on Linux, as
 * the fake host says, unless `platform` names another OS. */
async function open(
  tools: ToolState[] | undefined,
  settings = true,
  platform?: "mac" | "windows",
): Promise<Harness> {
  const harness = await mountRoom({
    next: { parts: ["app"], query: () => Promise.reject(new Error("not scripted")) },
    ...(settings ? { openSettings: () => {} } : {}),
    ...(platform === undefined ? {} : { platform }),
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
const play = async (state: BrowserPolicyState) => {
  await act(async () => h!.transport.appEvent({ browser_policy: { state } }));
};
/** The recheck_browser_policy commands dispatched so far. */
const rechecks = () =>
  h!.transport.only("dispatch").filter((c) => "recheck_browser_policy" in c.command.command);
const openPopover = async () => {
  await h!.click(line());
  await act(() => settled());
};
const described = (el: HTMLElement | null) =>
  document.getElementById(el?.getAttribute("aria-describedby") ?? "")?.textContent ?? null;

test("a header with tools shows their glyphs after the plan, parted by a dot, named in words and no visible words", async () => {
  h = await open([web("available"), browser(off("browser_not_installed"))]);
  expect(line()?.getAttribute("aria-label")).toBe("Web search: on. Browser: off, install Chrome.");
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
    [web(off("locked_off")), browser(off("locked_off"))],
    [
      ["web_search", "locked"],
      ["browser", "locked"],
    ],
    "Web search: off, managed by firm. Browser: off, managed by firm.",
  ],
  [[browser(off("locked_off"))], [["browser", "locked"]], "Browser: off, managed by firm."],
  [[browser(off("browser_not_installed"))], [["browser", "off"]], "Browser: off, install Chrome."],
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
    (el) => el.textContent === "Web search: on. Browser: off, install Chrome.",
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
    // the guarded browser's reasons never come for #870's shell browser: no words, no glyph
    browser(off("no_proxy")),
    browser(off("too_many_browsers")),
    browser(off("managed_by_firm")),
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
  [browser(off("browser_not_installed")), "Install Chrome"],
  [browser(off("browser_blocked_by_policy")), "Blocked by your firm"],
  [browser(off("browser_policy_unreadable")), "Couldn't check browser settings"],
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
    // nothing else in words: the name, the chip and the reason; with no Chrome, what to install in full; blocked by
    // the firm's policy, or a policy Clauseo couldn't check, the reason in full (that one, Try again)
    const offer =
      reason === "Install Chrome"
        ? "Install Google Chrome, then close and reopen Clauseo."
        : reason === "Blocked by your firm"
          ? "Your firm doesn't allow apps to control a browser on this computer, so Clauseo can't use " +
            "one for you."
          : reason === "Couldn't check browser settings"
            ? "Clauseo couldn't check this computer's browser settings, so it won't use a browser for " +
              "now. It checks again the next time it needs one." +
              "Try again"
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
  h = await open([web(off("locked_off")), browser(off("browser_not_installed"))]);
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

test.each([
  [
    undefined,
    "Install Chrome",
    "Install Google Chrome, then close and reopen Clauseo.",
    "install Chrome",
  ],
  [
    "mac",
    "Install Chrome",
    "Install Google Chrome, then close and reopen Clauseo.",
    "install Chrome",
  ],
  [
    "windows",
    "Install Chrome or Edge",
    "Install Google Chrome or Microsoft Edge, then close and reopen Clauseo.",
    "install Chrome or Edge",
  ],
] as const)(
  "no browser on the computer (%s): what to install, in a few words and in full, and nothing to press",
  async (platform, reason, full, inLine) => {
    h = await open([browser(off("browser_not_installed"))], true, platform);
    await openPopover();
    expect(said("browser", "tool-row-reason")).toBe(reason);
    expect(said("browser", "tool-row-full")).toBe(full);
    expect(row("browser")?.querySelector("button")).toBeNull();
    expect(row("browser")?.querySelector('[role="progressbar"]')).toBeNull();
    expect(line()?.getAttribute("aria-label")).toBe(`Browser: off, ${inLine}.`);
  },
);

test("Try again asks for the policy once and waits; a read that finds it clear says Ready for new conversations", async () => {
  h = await open([browser(off("browser_policy_unreadable"))]);
  await openPopover();
  const again = inRow("browser", "tool-recheck");
  expect(again?.textContent).toBe("Try again");
  await h.click(again);
  expect(rechecks()).toHaveLength(1);
  expect(inRow("browser", "tool-recheck")?.getAttribute("aria-busy")).toBe("true");
  await play("clear");
  // a reason kept, with a tick and a tooltip on when the browser comes
  expect(said("browser", "tool-row-reason")).toBe("Ready for new conversations");
  const ready = inRow("browser", "tool-row-reason")!;
  expect(ready.querySelector("svg")).not.toBeNull();
  expect(ready.getAttribute("tabindex")).toBe("0");
  expect(described(ready)).toBe("The browser comes with your next conversation.");
  expect(inRow("browser", "tool-row-full")).toBeNull();
  expect(inRow("browser", "tool-recheck")).toBeNull();
  // the line never changes for the read: the header still says it couldn't check
  expect(line()?.getAttribute("aria-label")).toBe("Browser: off, couldn't check browser settings.");
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

test("opened by a key, the popup takes the focus too, and an Enter right after opening asks for nothing", async () => {
  h = await open([browser(off("browser_policy_unreadable"))]);
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
  expect(inRow("browser", "tool-recheck")).not.toBeNull();
  await pressEnter();
  expect(rechecks()).toEqual([]);
  expect(inRow("browser", "tool-recheck")).not.toBeNull();
});

test("the popover keeps one width through the policy read's states", async () => {
  h = await open([browser(off("browser_policy_unreadable"))]);
  await openPopover();
  const classes = [popup()?.className];
  await play("checking");
  classes.push(popup()?.className);
  await play("clear");
  classes.push(popup()?.className);
  expect(classes[0]?.split(" ")).toContain("w-card-tile");
  expect(new Set(classes).size).toBe(1);
});
