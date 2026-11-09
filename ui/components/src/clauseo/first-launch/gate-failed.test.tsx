// @vitest-environment happy-dom
// A sign-in Codex couldn't confirm, or that now belongs to another account (R17), asks to sign in again; over a login
// Clauseo reuses from the lawyer's own app desktop refuses that, so the card offers another account instead.
import { act, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, test, vi } from "vitest";
import { fl, type FirstLaunchKey } from "../../lib/words-first-launch.ts";
import { Failed } from "./gate-failed.tsx";
import type { GateAct } from "./gate.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement | null = null;
let root: Root | null = null;

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  host = null;
});

async function mount(code: string, reused: boolean | undefined) {
  const onAct = vi.fn<(a: GateAct) => void>();
  const w = (key: FirstLaunchKey, slots?: Record<string, string | number>) => fl("en", key, slots);
  const node: ReactNode = (
    <Failed
      plan="chatgpt"
      view={{ code, at: "4:40 pm", reused }}
      w={w}
      way={(label, a) => (
        <button type="button" onClick={() => onAct(a)}>
          {label}
        </button>
      )}
      signIn={(label) => (
        <button type="button" onClick={() => onAct({ act: "sign-in" })}>
          {label}
        </button>
      )}
      instead={() => null}
      onAct={onAct}
    />
  );
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root?.render(node));
  return onAct;
}

const buttons = () => [...(host?.querySelectorAll("button") ?? [])];

for (const code of ["signin.unconfirmed", "signin.account_changed"]) {
  test(`${code} not reused: one Sign in button that signs in`, async () => {
    const onAct = await mount(code, undefined);
    expect(buttons().map((b) => b.textContent)).toEqual(["Sign in"]);
    await act(async () => buttons()[0]?.click());
    expect(onAct).toHaveBeenCalledWith({ act: "sign-in" });
    expect(host?.textContent).not.toContain("Sign-in didn't finish.");
  });

  test(`${code} reused: the different-account action instead of Sign in`, async () => {
    const onAct = await mount(code, true);
    expect(buttons().map((b) => b.textContent)).toEqual(["Sign in with a different account"]);
    await act(async () => buttons()[0]?.click());
    expect(onAct).toHaveBeenCalledTimes(1);
    expect(onAct).toHaveBeenCalledWith({ act: "different" });
  });
}

test("signin.account_changed says its own words, not the catch-all", async () => {
  await mount("signin.account_changed", false);
  expect(host?.textContent).toContain(
    "Your sign-in now belongs to a different account from the one you chose in Clauseo.",
  );
});
