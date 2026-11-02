// @vitest-environment happy-dom
// Versions in the real room over FakeTransport: opening it puts the timeline in the rail's place, and closing it brings
// the rail back.
import { afterEach, expect, test } from "vitest";
import { file, mountVersions, stepEntry, version, type VersionsHarness } from "./testing.ts";
import { THREAD, TURN } from "../../testing/stream-script.ts";

let h: VersionsHarness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
});

test("Versions stands in the rail's place while open, and closing it brings the rail back", async () => {
  h = await mountVersions(
    [version(1, "import"), version(2, "after_turn", { thread: THREAD, turn: TURN })],
    [stepEntry(5, { files: [file("brief.txt")], checkpoint_after: "cp2" })],
  );
  expect(h.host.querySelector("nav.room-rail")).not.toBeNull();
  await h.openVersions();
  expect(h.one("versions")).not.toBeNull();
  expect(h.host.querySelector("nav.room-rail")).toBeNull();
  const rows = h.all("version");
  expect(rows.map((r) => r.dataset["kind"])).toEqual(["after_turn", "import"]);
  expect(rows[0]?.textContent).toContain("brief.txt");
  await h.click(h.button("Close", h.one("versions")));
  expect(h.one("versions")).toBeNull();
  expect(h.host.querySelector("nav.room-rail")).not.toBeNull();
});
