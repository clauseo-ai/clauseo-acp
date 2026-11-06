// UD2 G6: the e2e build's scripted sign-in (E2E_FAKE_SIGNIN, crates/desktop/src/setup/fake_signin.rs)
// walked from the room. Claude starts installed (E2E_AGENTS_MANAGED) but out of `plans`, so its
// gate shows; Sign in plays Waiting, then signed in (`ok`) or failed (`fail`).
//
// Needs the real Windows e2e app: CI runs it in e2e-windows.yml with every app-windows spec
// (Playwright's `app` project, `pnpm -C ui/e2e run test`); it does not run headless.

import { expect } from "@playwright/test";
import { test } from "./fixture.ts";
import { openMatter } from "./flow.ts";
import { window } from "./selectors.ts";

const CLAUDE = "claude-acp";

test.describe("a scripted sign-in that succeeds", () => {
  test.use({ fakeSignIn: { [CLAUDE]: "ok" } });

  test("Sign in walks Waiting, then the room says it is signed in", async ({ app }) => {
    const w = window(app.page);
    await openMatter(w);
    const gate = app.page.getByTestId("signin");
    const state = app.page.getByTestId("signin-state");
    await expect(gate).toHaveAttribute("data-agent", "claude");
    await expect(state).toHaveAttribute("data-state", "signed-out");
    const signIn = gate.getByRole("button", { name: "Sign in", exact: true });
    await expect(signIn).toBeVisible();
    await expect(signIn).toBeEnabled();
    await signIn.click();
    await expect(state).toHaveAttribute("data-state", "waiting");
    await expect(app.page.getByTestId("signin-done")).toContainText("e2e@example.invalid");
    await expect(gate).toHaveCount(0);
  });
});

test.describe("a scripted sign-in that fails", () => {
  test.use({ fakeSignIn: { [CLAUDE]: "fail" } });

  test("Sign in walks Waiting, then the gate says it failed", async ({ app }) => {
    const w = window(app.page);
    await openMatter(w);
    const gate = app.page.getByTestId("signin");
    const state = app.page.getByTestId("signin-state");
    // the starting state: the sign-in card is up, signed out, before Sign in is pressed
    await expect(gate).toHaveAttribute("data-agent", "claude");
    await expect(state).toHaveAttribute("data-state", "signed-out");
    const signIn = gate.getByRole("button", { name: "Sign in", exact: true });
    await expect(signIn).toBeVisible();
    await expect(signIn).toBeEnabled();
    await signIn.click();
    await expect(state).toHaveAttribute("data-state", "waiting");
    await expect(state).toHaveAttribute("data-state", "signin.failed");
  });
});
