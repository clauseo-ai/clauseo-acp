// @vitest-environment happy-dom
// The stand-ins seam: the room takes stand-ins only as passed to it, never from methods a transport happens to carry;
// and each stand-in keeps the type HostTransport gives a method of the same name, so H11's openFile and reveal can
// replace them unchanged.
import { afterEach, expect, expectTypeOf, test } from "vitest";
import type { HostTransport } from "../../transport/host-transport.ts";
import type { FileRefId } from "../../transport/protocol.ts";
import type { StandIns } from "./stand-ins.ts";
import { mountVersions, version, type VersionsHarness } from "./testing.ts";

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

test("a transport that carries reveal lends Versions nothing; the stand-ins passed to the room do", async () => {
  const shown: unknown[] = [];
  h = await mountVersions([version(1, "import")], [], {
    carries: { reveal: async (t: unknown) => void shown.push(t) },
  });
  await h.openVersions();
  await h.click(document.body.querySelector(".ver-foot"));
  expect(showButton()).toBeUndefined();
  await h.unmount();

  h = await mountVersions([version(1, "import")], [], {
    standIns: { reveal: async (t) => void shown.push(t) },
  });
  await h.openVersions();
  await h.click(document.body.querySelector(".ver-foot"));
  await h.click(showButton());
  expect(shown).toEqual(["app_data"]);
});
