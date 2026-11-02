// @vitest-environment happy-dom
// The stand-ins seam: without stand-ins the room lends Versions the transport's own openFile and reveal (H11), and
// stand-ins passed to the room replace them; each stand-in keeps the type HostTransport gives a method of the same
// name.
import { afterEach, expect, expectTypeOf, test } from "vitest";
import type { HostTransport } from "../../transport/host-transport.ts";
import type { FileRefId } from "../../transport/protocol.ts";
import type { StandIns } from "./stand-ins.ts";
import { THREAD, TURN, uuid } from "../../testing/stream-script.ts";
import { fakeNext } from "../fake-next.ts";
import { file, mountVersions, stepEntry, version, type VersionsHarness } from "./testing.ts";

/** Exactly the same type, both ways. */
type Same<A, B> =
  (<T>() => T extends A ? 1 : 2) extends <T>() => T extends B ? 1 : 2 ? true : false;

/** The stand-ins whose type differs from a same-named member of `Host`. */
type Drift<Host> = {
  [K in keyof StandIns & keyof Host]: Same<NonNullable<StandIns[K]>, Host[K]> extends true
    ? never
    : K;
}[keyof StandIns & keyof Host];

test("each stand-in has the type of HostTransport's method of the same name, where it has one", () => {
  expectTypeOf<Drift<HostTransport>>().toEqualTypeOf<never>();
  // H11's planned shapes fit
  expectTypeOf<
    Drift<{
      openFile(file: FileRefId): Promise<void>;
      reveal(target: { matter: string } | "app_data"): Promise<void>;
    }>
  >().toEqualTypeOf<never>();
  // and a drifted one is caught
  expectTypeOf<
    Drift<{ openFile(file: FileRefId): Promise<boolean>; diskUse(m: string): Promise<number> }>
  >().toEqualTypeOf<"openFile" | "diskUse">();
});

let h: VersionsHarness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
});

const showButton = () =>
  [...document.body.querySelectorAll(".ver-where button")].find((b) =>
    (b.textContent ?? "").startsWith("Show in"),
  );

test("without stand-ins Show reveals through the transport; stand-ins passed to the room replace it", async () => {
  h = await mountVersions([version(1, "import")], []);
  await h.openVersions();
  await h.click(document.body.querySelector(".ver-foot"));
  await h.click(showButton());
  expect(h.transport.only("reveal").map((c) => c.target)).toEqual(["app_data"]);
  await h.unmount();

  const shown: unknown[] = [];
  h = await mountVersions([version(1, "import")], [], {
    standIns: { reveal: async (t) => void shown.push(t) },
  });
  await h.openVersions();
  await h.click(document.body.querySelector(".ver-foot"));
  await h.click(showButton());
  expect(shown).toEqual(["app_data"]);
  expect(h.transport.only("reveal")).toEqual([]);
});

test("without stand-ins Open now opens the file through the transport's open_file, by the wall's file ref", async () => {
  const ref = uuid(1, 8);
  h = await mountVersions(
    [
      version(1, "import"),
      version(2, "after_turn", { thread: THREAD, turn: TURN }),
      version(3, "outside_edits", { files: [file("exhibit.pdf")] }),
    ],
    [stepEntry(5, { files: [file("exhibit.pdf")], checkpoint_after: "cp2" })],
    {
      query: (q) =>
        typeof q === "object" && q.compare_versions !== undefined
          ? { file_changes: [file("exhibit.pdf")] }
          : undefined,
      next: fakeNext({
        matters: () => [],
        papers: [{ file: ref, path: "exhibit.pdf", size: 10, modified: 0 }],
        parts: ["papers"],
      }),
    },
  );
  await h.openVersions();
  await h.lookAt("cp2");
  await h.click(h.button("Compare with now"));
  await h.click(h.button("Open now"));
  expect(h.transport.only("open_file")).toEqual([{ call: "open_file", file: ref }]);
});
