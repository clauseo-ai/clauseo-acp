// The first screen's Open the folder for a folder Claude can't start in (FR30; folder-backslash.ts, locate-warning.tsx):
// on macOS and Linux a folder picked whose path has a backslash in a folder's name waits for the step, as New matter's
// warning and Locate do: Choose another folder, which picks again, or Open anyway, which makes it a matter, named by
// its own name, backslash and all. On Windows, or for a plain path, it is made at once.
// @vitest-environment happy-dom
import { expect, test } from "vitest";
import { byId, mountFirstRun, press, sent } from "./rig.tsx";

/** The file manager the step names: Finder on the Mac, "your file manager" on Linux. */
const FILES = { mac: "Finder", linux: "your file manager" } as const;
const SLASHED = { folder: "folder-ref-1", display: "Documents/Clauseo\\ Test" };
const PLAIN = { folder: "folder-ref-2", display: "Documents/Matters/Rathi Steels" };

/** The first screen over a host on `platform` whose picker gives `picks` in turn. */
async function mount(platform: "mac" | "linux" | "windows", picks: (typeof SLASHED)[]) {
  const transport = await mountFirstRun({ matters: [], platform });
  let n = 0;
  transport.pickFolder = async () => picks[n++] ?? null;
  return transport;
}

const created = (transport: Awaited<ReturnType<typeof mount>>) => sent(transport, "create_matter");

test.each(["mac", "linux"] as const)(
  "on %s, a folder whose name has a backslash waits for the step; Choose another folder picks again",
  async (platform) => {
    const transport = await mount(platform, [SLASHED, PLAIN]);
    await press(byId("open-folder"));
    const step = byId("first-backslash-warning");
    expect(step?.textContent).toContain("This folder's name has a backslash (\\).");
    expect(step?.textContent).toContain(
      `Claude can't work in a folder like this; ChatGPT can. Rename it in ${FILES[platform]} first, or choose another folder.`,
    );
    expect(byId("first-backslash-choose")?.textContent).toBe("Choose another folder");
    expect(byId("first-backslash-anyway")?.textContent).toBe("Open anyway");
    expect(created(transport)).toEqual([]);
    await press(byId("first-backslash-choose"));
    expect(byId("first-backslash-warning")).toBeNull();
    expect(created(transport)).toEqual([
      { name: "Rathi Steels", source: { existing_folder: { folder: "folder-ref-2" } } },
    ]);
  },
);

test.each(["mac", "linux"] as const)(
  "on %s, Open anyway makes the folder with a backslash a matter, by its own name",
  async (platform) => {
    const transport = await mount(platform, [SLASHED]);
    await press(byId("open-folder"));
    await press(byId("first-backslash-anyway"));
    expect(byId("first-backslash-warning")).toBeNull();
    expect(created(transport)).toEqual([
      { name: "Clauseo\\ Test", source: { existing_folder: { folder: "folder-ref-1" } } },
    ]);
  },
);

test("a folder above the one picked has the backslash: the step names it", async () => {
  await mount("mac", [{ folder: "folder-ref-3", display: "Documents/Old\\Work/Rathi Steels" }]);
  await press(byId("open-folder"));
  expect(byId("first-backslash-warning")?.textContent).toContain(
    'The folder "\u2068Old\\Work\u2069" in this folder\'s path has a backslash (\\).',
  );
});

test.each([
  ["windows", { folder: "folder-ref-1", display: "D:\\Matters\\Rathi Steels" }, "Rathi Steels"],
  ["mac", PLAIN, "Rathi Steels"],
  ["linux", PLAIN, "Rathi Steels"],
] as const)("on %s, %j is made a matter at once, with no step", async (platform, picked, name) => {
  const transport = await mount(platform, [picked]);
  await press(byId("open-folder"));
  expect(byId("first-backslash-warning")).toBeNull();
  expect(created(transport).map((c) => c.name)).toEqual([name]);
});
