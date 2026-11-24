// @vitest-environment happy-dom
// The place's entry point for the home card (open-library.ts): opened at CASE_LAW_START, it stands
// on Connectors with Clauseo Case Law picked, its detail beside the list.
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, test } from "vitest";
import { FakeTransport } from "../transport/fake-transport.ts";
import { fixedAddons } from "./addons-source.ts";
import { caseLawStatus, interimItems } from "./addons-interim.ts";
import { Library } from "./library.tsx";
import { CASE_LAW_START } from "./open-library.ts";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

afterEach(() => document.body.replaceChildren());

test("opened at CASE_LAW_START, the place shows Connectors with Case Law picked", async () => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const listed = { available: true, locked: false, connectors: [] };
  const items = interimItems([], null, listed, true);
  await act(async () =>
    root.render(
      <Library
        transport={new FakeTransport({})}
        source={fixedAddons(items)}
        names={new Map()}
        plan="claude"
        start={CASE_LAW_START}
      />,
    ),
  );
  const chosen = host.querySelector('[role="tab"][aria-selected="true"]');
  expect(chosen?.textContent).toBe("Connectors");
  expect(host.querySelector('[data-testid="case-law-card"]')?.getAttribute("aria-current")).toBe(
    "true",
  );
  expect(host.querySelector(".lib-detail-name")?.textContent).toBe("Clauseo Case Law");
  // the host listing no connector is the unreachable state
  expect(caseLawStatus(listed, true)).toBe("unreachable");
  await act(async () => root.unmount());
});
