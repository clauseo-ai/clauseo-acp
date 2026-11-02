// What the hostile DOM tests share (hostile-all.test.tsx and hostile-stream-*.test.tsx): the hostile cases,
// rendered by the renderer in happy-dom, with every check of G4's conformance page that a DOM can show: no
// forbidden element, links and images only through our components, allowed protocols only, every id prefixed,
// no event-handler attribute, raw HTML visible as text, the link-host hint, no clobbered global, every link
// click cancelled and handed to the host. Each case is also streamed in 16-character deltas (one character for
// the tag cases that ask), checked after every delta. Every allowlisted inline element carries no attribute, and
// each case's `tags` counts hold. What needs a real engine (CSP and Trusted Types violations, a trusted click not
// navigating, network requests, a payload running) is the private harness's, which imports hostile-cases.ts too.
//
// Two states of the entity table: before it has loaded (its prefetch is held back by each test file's vi.mock of
// entities.ts) and after, since a character reference outside the common set decodes only in the second state.
// The table decodes named character references and nothing else, so a case with no `&` in its source renders the
// same in both: every case gets every check before the table loads, and after it the cases with an `&`
// (hasReference) get them all again, while hostile-all.test.tsx checks that every other case's finished and
// streaming renders are the same HTML as before (and so pass the same checks, its streamed prefixes too).
//
// The checks are split over several files so vitest runs them in parallel: hostile-all.test.tsx renders all the
// cases at once and streams the cases with an `&` once the table has loaded, and each hostile-stream-<k>.test.tsx
// streams every STREAM_FILES-th case (streamSlice) before it has, so every case is streamed by exactly one file and
// a case added to hostile-cases.ts is picked up with nothing to edit.
import { act } from "react";
import type { ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { afterAll, beforeAll, describe, expect, test } from "vitest";
import { characterEntitiesLoaded, loadEntities } from "./entities.ts";
import { CASES } from "./hostile-cases.ts";
import type { HostileCase } from "./hostile-cases.ts";
import { MarkdownHostContext } from "./host.ts";
import type { MarkdownHost } from "./host.ts";
import { sliceOfThisFile } from "./hostile-slice-fixture.ts";
import { Markdown } from "./markdown.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const FORBIDDEN =
  "script,iframe,frame,object,embed,style,link,meta,form,input,noscript,base,template,textarea,svg,math,button";
const LINK_OK = /^(?:https?:|mailto:|#)/i;
const IMAGE_OK = /^(?:clauseo-file:|data:image\/(?:png|jpeg|gif|webp);base64,)/i;
const DELTA = 16;
/** The approved inline elements; none may carry an attribute (a kbd only the key cap's own class). */
const INLINE_TAGS = "sup,sub,u,ins,del,s,mark,small,br,kbd";

/** Every failure G4's checkContainer would report for one container. */
const MAX_DOM_DEPTH = 160;

/** How many hostile-stream-<k>.test.tsx files there are; file k streams the cases at k-1, k-1 + STREAM_FILES, ... */
export const STREAM_FILES = 3;

/** The cases hostile-stream-<file>.test.tsx streams (file counts from 1), for hostile-all.test.tsx's guard. */
export function streamSlice(file: number): HostileCase[] {
  return CASES.filter((_, index) => index % STREAM_FILES === file - 1);
}

/** How many elements deep `el`'s deepest descendant is (an iterative walk). */
function depthOf(el: Element): number {
  let deepest = 0;
  const stack: [Element, number][] = [[el, 0]];
  for (let item = stack.pop(); item; item = stack.pop()) {
    const [node, depth] = item;
    deepest = Math.max(deepest, depth);
    for (const child of node.children) stack.push([child, depth + 1]);
  }
  return deepest;
}

export function failuresIn(label: string, el: Element): string[] {
  const failures: string[] = [];
  for (const bad of el.querySelectorAll(FORBIDDEN))
    failures.push(`${label}: <${bad.tagName.toLowerCase()}> rendered`);
  for (const a of el.querySelectorAll("a")) {
    if (a.getAttribute("data-clauseo") !== "link")
      failures.push(`${label}: <a> not from the link component`);
    const href = a.getAttribute("href");
    if (href !== null && !LINK_OK.test(href))
      failures.push(`${label}: link href ${href.slice(0, 60)}`);
  }
  for (const img of el.querySelectorAll("img")) {
    if (img.getAttribute("data-clauseo") !== "image")
      failures.push(`${label}: <img> not from the image component`);
    const src = img.getAttribute("src") ?? "";
    if (!IMAGE_OK.test(src)) failures.push(`${label}: image src ${src.slice(0, 60)}`);
  }
  for (const node of el.querySelectorAll("[id]")) {
    if (!node.id.startsWith("user-content-"))
      failures.push(`${label}: id "${node.id}" without the user-content- prefix`);
  }
  // What nesting.ts cuts never shows, and no tree gets past its depth limit (MAX_TREE_DEPTH, 128 hast
  // levels, plus the components' own wrappers).
  if (/[\uFDD1-\uFDE0]/.test(el.textContent ?? ""))
    failures.push(`${label}: a stand-in for a cut character shows`);
  const depth = depthOf(el);
  if (depth > MAX_DOM_DEPTH) failures.push(`${label}: ${depth} elements deep`);
  for (const node of el.querySelectorAll("*")) {
    for (const attribute of node.getAttributeNames()) {
      if (/^on/i.test(attribute)) failures.push(`${label}: event handler attribute ${attribute}`);
    }
  }
  for (const node of el.querySelectorAll(INLINE_TAGS)) {
    const own =
      node.tagName === "KBD" ? ["class"] : node.tagName === "SUP" ? ["data-footnote-pending"] : [];
    for (const attribute of node.getAttributeNames())
      if (!own.includes(attribute))
        failures.push(`${label}: <${node.tagName.toLowerCase()} ${attribute}> kept an attribute`);
  }
  return failures;
}

export const opened: string[] = [];
const references: string[] = [];
const HOST: MarkdownHost = {
  openExternal: (url) => opened.push(url),
  openReference: (kind, id) => references.push(`${kind}:${id}`),
};
export const pwned: string[] = [];
export const g4 = globalThis as { __g4pwn?: (id: string) => void; __g4clobber?: unknown };

/** Whether `c`'s source holds a character reference (or anything else that starts with `&`). */
export const hasReference = (c: HostileCase) => c.markdown.includes("&");

/** A React root in the document, which the hostile cases render into. */
export type Surface = {
  /** The root's container. */
  el: HTMLDivElement;
  /** Renders `children` into the root, inside the host context. */
  render(children: ReactNode): void;
};

/** Sets the payloads' __g4pwn for the file's tests, and removes it after. */
export function catchPayloads(): void {
  beforeAll(() => {
    g4.__g4pwn = (id) => pwned.push(id);
  });
  afterAll(() => {
    delete g4.__g4pwn;
  });
}

/** Mounts a surface for the file's tests (the returned function gives it), and unmounts it after. */
export function mountSurface(): () => Surface {
  let surface: (Surface & { unmount(): void }) | undefined;
  beforeAll(() => {
    const el = document.createElement("div");
    document.body.append(el);
    const root = createRoot(el);
    surface = {
      el,
      render: (children) =>
        act(() => root.render(<MarkdownHostContext value={HOST}>{children}</MarkdownHostContext>)),
      unmount: () => {
        act(() => root.unmount());
        el.remove();
      },
    };
  });
  afterAll(() => surface?.unmount());
  return () => {
    if (!surface) throw new Error("the surface is not mounted");
    return surface;
  };
}

/** Every case in `cases`, each in its own `<section data-case>`. */
export function casesNode(cases: readonly HostileCase[], streaming: boolean): ReactNode {
  return cases.map((c) => (
    <section key={c.id} data-case={c.id}>
      <Markdown text={c.markdown} streaming={streaming} />
    </section>
  ));
}

/** Loads the entity table (mounted answers re-render when it arrives) and checks it has. */
export async function loadTable(): Promise<void> {
  await act(() => loadEntities());
  expect(characterEntitiesLoaded()).toBe(true);
}

/**
 * The prefix lengths a case streams through, 16 characters apart. The 20,000-character line is one paragraph
 * that is parsed again on every delta (tens of milliseconds a delta near its end here, about a minute for all
 * 1,250 deltas); it streams its first 1,000 and its last 320 characters (its middle repeats the same 67 characters).
 */
function ends(length: number, delta = DELTA): number[] {
  const all: number[] = [];
  for (let end = delta; end < length + delta; end += delta) all.push(Math.min(end, length));
  return length <= 2_000 ? all : all.filter((end) => end <= 1_000 || end >= length - 320);
}

/** Every failure of `c` streamed into `surface` in its deltas, checked after each; the surface is emptied after. */
export function streamFailures(surface: Surface, c: HostileCase): string[] {
  const failures: string[] = [];
  for (const end of ends(c.markdown.length, c.delta)) {
    surface.render(
      <section data-case={c.id}>
        <Markdown text={c.markdown.slice(0, end)} streaming />
      </section>,
    );
    failures.push(...failuresIn(`${c.id} at ${end}`, surface.el));
    // Text that has fully arrived shows, mid-stream too: nothing half-arrived hides what follows it.
    const shown = surface.el.textContent ?? "";
    for (const literal of c.streamingText ?? c.text ?? []) {
      if (c.markdown.slice(0, end).includes(literal) && !shown.includes(literal))
        failures.push(`${c.id} at ${end}: "${literal}" arrived but does not show`);
    }
  }
  surface.render(null);
  return [...new Set(failures)];
}

/**
 * The tests of a hostile-stream-<k>.test.tsx: every case of streamSlice(k), k from the file's own name, streamed in
 * its deltas with G4's DOM checks after every delta, before the entity table has loaded. Called with nothing, so a
 * file can neither pick another slice nor drop a case from its own.
 */
export function streamThisSlice(): void {
  const cases = streamSlice(sliceOfThisFile("hostile-stream-", ".test.tsx", STREAM_FILES));
  catchPayloads();
  const surface = mountSurface();
  describe("entity table not loaded", () => {
    beforeAll(() => {
      expect(characterEntitiesLoaded()).toBe(false);
    });

    test.each(cases.map((c) => [c.id, c] as const))(
      "%s streamed in 16-character deltas stays clean at every step",
      (_, c) => {
        expect(streamFailures(surface(), c)).toEqual([]);
        expect(pwned).toEqual([]);
      },
      30_000,
    );
  });
}
