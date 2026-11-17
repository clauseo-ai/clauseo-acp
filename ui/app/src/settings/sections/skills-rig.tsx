// The settings window over FakeTransport for Settings › Skills' tests: open at #skills over a FakeSkills host
// (skills-fake.ts), with helpers to press and read. Not a test itself.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { settled } from "@clauseo/components/lib/late.ts";
import { connectDisplay } from "../../display/connect.ts";
import type { Settings } from "../../room/host-next.ts";
import { FakeTransport } from "../../transport/fake-transport.ts";
import { SettingsApp } from "../settings-app.tsx";
import { FakeSkills } from "./skills-fake.ts";
import type { SkillPack } from "./skills-contract.ts";

export const SETTINGS: Settings = {
  ui: { theme: "system", font_size: 16, language: "en", lite: "automatic", motion: "system" },
  locked: [],
};

export type Rig = Awaited<ReturnType<typeof mount>>;

export async function mount(packs: readonly SkillPack[]) {
  history.replaceState(null, "", "/settings.html#skills");
  const skills = new FakeSkills(packs);
  const transport = new FakeTransport({
    next: {
      query: (query) => {
        if (query === "get_settings") return { settings: SETTINGS };
        throw new Error("not scripted");
      },
    },
    ...skills.host(),
  });
  const store = connectDisplay(transport).store;
  const el = document.createElement("div");
  document.body.append(el);
  const root: Root = createRoot(el);
  const settle = async () => {
    for (let i = 0; i < 3; i += 1) await act(async () => settled());
  };
  await act(async () =>
    root.render(<SettingsApp store={store} transport={transport} windowControls={null} />),
  );
  await settle();
  const pane = () => el.querySelector("main.set-pane") as HTMLElement;
  const q = <E extends Element = HTMLElement>(sel: string) => pane().querySelector<E>(sel);
  const all = (sel: string) =>
    [...pane().querySelectorAll<HTMLElement>(sel)].map((n) => n.textContent ?? "");
  const click = async (target: HTMLElement | null | undefined) => {
    if (target === null || target === undefined) throw new Error("nothing to click");
    await act(async () => target.click());
    await settle();
  };
  /** A row's element by the pack's name. */
  const row = (name: string) =>
    [...pane().querySelectorAll<HTMLElement>(".set-row")].find(
      (r) => r.querySelector('[data-testid="skills-row"] p')?.textContent === name,
    );
  return {
    el,
    transport,
    skills,
    pane,
    q,
    settle,
    click,
    row,
    nav: () =>
      [...el.querySelectorAll(".set-nav-row .set-nav-name")].map((n) => n.textContent ?? ""),
    names: () => all('[data-testid="skills-row"] > p:first-child'),
    /** Each row's source line, or "" where it has none. */
    sources: () =>
      [...pane().querySelectorAll('[data-testid="skills-row"]')].map(
        (r) => r.querySelector('[data-testid="skills-source"]')?.textContent ?? "",
      ),
    /** Each row's state line, or "" where it has none. */
    states: () =>
      [...pane().querySelectorAll('[data-testid="skills-row"]')].map(
        (r) => r.querySelector('[data-testid="skills-state"]')?.textContent ?? "",
      ),
    unmount: async () => {
      await act(async () => root.unmount());
      el.remove();
      store.dispose();
    },
  };
}
