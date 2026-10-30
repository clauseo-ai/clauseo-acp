import { expect, test } from "vitest";
import * as icons from "./icons.tsx";
import { ICON_NAMES } from "./names.ts";

test("names.ts lists every icon in icons.tsx, once, in its order", () => {
  expect(ICON_NAMES.map((n) => n.component)).toEqual(Object.keys(icons));
});
