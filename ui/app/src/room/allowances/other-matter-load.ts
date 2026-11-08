// R18: the list of the other matters (other-matter.tsx) in a chunk of its own, loaded on the menu's press: it takes
// the dialog's primitives, which the first load lacks (vite.config.ts's PRELOADED_CHUNKS). Written with
// createElement, so this file holds no component file's JSX, as note-load.ts is.
import { late } from "@clauseo/components/lib/late.ts";
import { createElement, lazy, Suspense, type ComponentProps } from "react";

type Module = typeof import("./other-matter.tsx");

const Part = lazy(() =>
  late(import("./other-matter.tsx")).then((m) => ({ default: m.OtherMatterDialog })),
);

/** The list, once its chunk is in; nothing before. */
export const LazyOtherMatter = (props: ComponentProps<Module["OtherMatterDialog"]>) =>
  createElement(Suspense, { fallback: null }, createElement(Part, props));
