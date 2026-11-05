import { expect, test, vi } from "vitest";

// Tauri's window module, replaced: the current window's methods record what the page asks of it.
const asked: string[] = [];
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    minimize: async () => void asked.push("minimise"),
    toggleMaximize: async () => void asked.push("toggleMaximise"),
    close: async () => void asked.push("close"),
    isMaximized: async () => true,
  }),
}));

const { tauriWindowControls } = await import("./window-controls.ts");

test("the first call loads Tauri's window module, and the controls answer and act on the current window", async () => {
  const controls = tauriWindowControls();
  expect(await controls.isMaximised()).toBe(true);
  await controls.minimise();
  await controls.toggleMaximise();
  await controls.close();
  expect(asked).toEqual(["minimise", "toggleMaximise", "close"]);
});
