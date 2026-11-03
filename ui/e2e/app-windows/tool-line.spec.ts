// The tool line over the real host: R21 PR-B's host fills each thread's HeaderEntry.tools once the session call
// answers (crates/host/src/actor/header_tools.rs, show_held), and the status line says them after the plan. In an
// e2e launch the agents are `claude-acp` and `codex-acp`, so each has a kind (AgentKind::of) and gets a web search
// state; `tools.web_search` is on by default and the fixture sets no `tools.*`, and no CLAUDE_CODE_USE_BEDROCK login
// is set, so web search is Available for both. The browser's state comes only from a profile hook's `browser` entry;
// the desktop app installs no hook yet, so today there is no Browser row, and when there is one it is one of the
// browser lines (the machine decides: no proxy before stage C, no Chrome, or off by default off macOS).

import { expect, type Locator, type Page } from "@playwright/test";
import { test } from "./fixture.ts";
import { expectTurnEnded, newConversation, openMatter, sendPrompt } from "./flow.ts";
import { window, type Window } from "./selectors.ts";

/** The web search row's words while it is on (ui/components/src/lib/words-tools.ts). */
const WEB_ON = "On. Clauseo can search the web and read pages.";

/** Every first line the browser row can say (words-tools.ts), whatever the machine's browser state. */
const BROWSER_LINES = [
  "On. Clauseo can open web pages in its own browser.",
  "Off. You turned it off in Settings.",
  "Off. Managed by your organisation.",
  "Off. Your organisation chooses the tools Claude has on this computer, so Clauseo adds none.",
  "Off. Your organisation chooses the tools ChatGPT has on this computer, so Clauseo adds none.",
  "Off until Clauseo can check each website the browser opens.",
  "Off. Other conversations are using every browser this computer can run. One frees up when another conversation closes.",
  "Off. There's no Chrome on this computer.",
];

/** The status line says web search is on, and its popover's rows say where each tool stands. */
async function expectWebSearchOn(w: Window, page: Page): Promise<void> {
  // "Web search on", or "Web search and browser on" where the machine gives the browser too
  await expect(w.toolLine).toContainText("Web search");
  await expect(w.toolLine).toHaveText(/^Web search (?:and browser )?on$/);
  await w.toolLine.click();
  await expect(w.toolPopover).toBeVisible();
  const web: Locator = w.toolRow("web_search");
  await expect(web).toContainText("Web search");
  await expect(w.toolRowLines(web)).toHaveText([WEB_ON]);
  const browser = w.toolRow("browser");
  if ((await browser.count()) > 0) {
    await expect(browser).toContainText("Browser");
    const first = await w.toolRowLines(browser).first().textContent();
    expect(BROWSER_LINES).toContain(first);
  }
  // Escape closes it, wherever the popover put the focus
  await page.keyboard.press("Escape");
  await expect(w.toolPopover).toHaveCount(0);
}

test.describe("Claude", () => {
  test.use({ scenario: "claude-hindi-answer" });

  test("once the session answers, the status line says web search is on, and its row says so", async ({
    app,
  }) => {
    const w = window(app.page);
    await openMatter(w);
    // claude-hindi-answer answers any prompt with its recording
    await sendPrompt(w, "दो मसौदा आपूर्ति समझौतों में दायित्व सीमा खंड की तुलना कीजिए।");
    await expectTurnEnded(w, "end_turn");
    await expectWebSearchOn(w, app.page);
  });
});

test.describe("Codex", () => {
  test.use({ scenario: "prompt-stream", otherAgents: { "codex-acp": "codex-hook-trusted" } });

  test("a conversation asked with ChatGPT says web search is on too", async ({ app }) => {
    const w = window(app.page);
    await openMatter(w);
    // as codex-thread.spec.ts: a Claude conversation first, then a new one asked with ChatGPT
    await sendPrompt(w, "Summarise the indemnity clause");
    await expectTurnEnded(w, "end_turn");
    await newConversation(w);
    await w.planOption("ChatGPT").click();
    await expect(w.planOption("ChatGPT")).toHaveAttribute("aria-pressed", "true");
    await sendPrompt(w, "Are you ready?");
    // only codex-hook-trusted says it: the thread ran as codex-acp
    await expect(w.agentMessages.last()).toHaveText("Guarded.");
    await expectTurnEnded(w, "end_turn");
    await expect(w.statusPlan).toHaveText("Uses your ChatGPT plan");
    await expectWebSearchOn(w, app.page);
  });
});
