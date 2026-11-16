// A sign-in pressed on the first screen before Claude is installed: the e2e build's scripted install
// (E2E_FAKE_INSTALL, crates/desktop/src/setup/fake_install.rs) starts Claude missing, so the
// install line in Claude's row at the foot offers "Sign in with Claude" (ChatGPT's row stands beside it, UD1-94); the press starts the install, desktop keeps it
// (crates/desktop/src/setup/backend/pending.rs) and runs it at Ready, where the scripted sign-in
// (E2E_FAKE_SIGNIN) plays. The matter opened afterwards shows the account, with no gate.
//
// Needs the real Windows e2e app: CI runs it in e2e-windows.yml with every app-windows spec
// (Playwright's `app` project, `pnpm -C ui/e2e run test`); it does not run headless.

import { expect } from "@playwright/test";
import { test } from "./fixture.ts";
import { window } from "./selectors.ts";

const CLAUDE = "claude-acp";

test.describe("a sign-in pressed before Claude is installed", () => {
  test.use({ firstRun: true, fakeInstall: [CLAUDE], fakeSignIn: { [CLAUDE]: "ok" } });

  test("Sign in on the first screen installs Claude, then signs in", async ({ app }) => {
    const page = app.page;
    const w = window(page);
    // both plans have a row at the foot, Claude's then ChatGPT's: this spec walks Claude's
    const row = page.locator(`[data-testid="plan-row"][data-agent="${CLAUDE}"]`);
    const foot = row.getByTestId("install-foot");
    // the starting state: the first screen, Claude missing, its download's size and Sign in with Claude
    await expect(foot).toHaveAttribute("data-state", "missing");
    // the line says it twice: once for screen readers (the row's sr-only status) and once on screen
    await expect(page.locator(`[data-testid="install-said"][data-agent="${CLAUDE}"]`)).toHaveText(
      /^Claude · needs a download of about \d+ MB$/,
    );
    await expect(
      row
        .getByText("needs a download of about", { exact: false })
        .and(page.locator(":not(.sr-only)")),
    ).toBeVisible();
    const signIn = foot.getByRole("button", { name: "Sign in with Claude", exact: true });
    await expect(signIn).toHaveAttribute("data-testid", "install-sign-in");
    await expect(signIn).toBeEnabled();
    await signIn.click();
    // the press starts the install: the line follows it, then gives way to "Claude is ready"
    await expect(foot).toHaveAttribute("data-state", "downloading");
    await expect(foot).toHaveCount(0);
    await expect(row.getByText("Claude is ready", { exact: true })).toBeVisible();
    // the kept press ran at Ready: the matter opened now is signed in, with no gate
    await page.getByRole("button", { name: "Open the folder for a matter" }).click();
    await expect(page.getByTestId("signin-done")).toContainText("e2e@example.invalid");
    await expect(w.signIn).toHaveCount(0);
    await expect(w.composer).toBeVisible();
    await expect(w.composer).toBeEnabled();
  });
});
