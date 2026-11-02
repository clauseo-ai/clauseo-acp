// @vitest-environment happy-dom
// Every hostile case rendered at once, finished and streaming, with G4's DOM checks, the link clicks and the
// link-host hint, in both states of the entity table; once it has loaded, the cases with an `&` streamed too, and
// every other case's renders checked to be the same as before it loaded (hostile-dom-fixture.tsx says why, what
// is checked and where the rest is).
import { act } from "react";
import { beforeAll, describe, expect, test, vi } from "vitest";
import { characterEntitiesLoaded } from "./entities.ts";
import { CASES } from "./hostile-cases.ts";
import { takesWholeSlice } from "./hostile-slice-fixture.ts";
import {
  casesNode,
  catchPayloads,
  failuresIn,
  g4,
  hasReference,
  loadTable,
  mountSurface,
  opened,
  pwned,
  STREAM_FILES,
  streamFailures,
  streamSlice,
} from "./hostile-dom-fixture.tsx";

// The component's idle prefetch would load the table part-way through the first state.
vi.mock("./entities.ts", async (original) => ({
  ...(await original<typeof import("./entities.ts")>()),
  prefetchEntities: () => {},
}));

catchPayloads();
/** Every case, finished and streaming: rendered once in each state, read by every check of that state. */
const finished = mountSurface();
const streamed = mountSurface();
const surface = (streaming: boolean) => (streaming ? streamed() : finished());
/** The streaming cases with an `&`, once the table has loaded. */
const stream = mountSurface();
/** A fresh render of every case once the table has loaded, beside the before-table HTML. */
const fresh = mountSurface();

function section(streaming: boolean, id: string, from = surface(streaming)): Element {
  const el = from.el.querySelector(`[data-case="${id}"]`);
  if (!el) throw new Error(`${id} not rendered`);
  return el;
}

/**
 * `el`'s HTML, with each answer's scope (host.ts's scopeOf of React's useId, `md_r_<n>_`, which a new mount numbers
 * anew) written as `md-scope`.
 */
const html = (el: Element) => el.innerHTML.replaceAll(/\bmd_r_[0-9a-z]+_/g, "md-scope");

/** The cases without an `&`, finished and streaming, as they rendered before the table loaded. */
const before = new Map<string, string>();

// The first state's renders, and `before` from them, for the whole file: every test below can run alone (`-t`).
beforeAll(() => {
  expect(characterEntitiesLoaded()).toBe(false);
  for (const streaming of [true, false]) {
    surface(streaming).render(casesNode(CASES, streaming));
    for (const c of CASES.filter((c) => !hasReference(c)))
      before.set(`${c.id} (streaming ${streaming})`, html(section(streaming, c.id)));
  }
  // Two renders of the corpus: what the per-render tests had, 30 s, not the hooks' default 10 s.
}, 30_000);

test("the corpus holds G4's 28 cases and the ones added since, each with its own id", () => {
  expect(CASES[27]?.id).toBe("unterminated-fence");
  expect(CASES).toHaveLength(109);
  expect(new Set(CASES.map((c) => c.id)).size).toBe(CASES.length);
});

test("the stream files between them stream every case once before the table loads; this file the rest after", () => {
  const sliced = Array.from({ length: STREAM_FILES }, (_, k) => streamSlice(k + 1)).flat();
  expect(sliced.map((c) => c.id).sort()).toEqual(CASES.map((c) => c.id).sort());
  // Files 1 to STREAM_FILES, each taking the slice its own name gives, whole.
  const sources = import.meta.glob<string>("./hostile-stream-*.test.tsx", {
    query: "?raw",
    import: "default",
    eager: true,
  });
  expect(Object.keys(sources).sort()).toEqual(
    Array.from({ length: STREAM_FILES }, (_, k) => `./hostile-stream-${k + 1}.test.tsx`),
  );
  for (const [file, source] of Object.entries(sources))
    expect(takesWholeSlice(source, "streamThisSlice", "streamSlice"), file).toBe(true);
  // The cases the second state streams: the character references, numeric ones too.
  expect(CASES.filter(hasReference).map((c) => c.id)).toEqual(
    expect.arrayContaining([
      "named-entities",
      "entity-sol-host",
      "js-link-entity",
      "tags-entities",
    ]),
  );
});

describe.each(["not loaded", "loaded"] as const)("entity table %s", (state) => {
  beforeAll(async () => {
    if (state === "not loaded") return expect(characterEntitiesLoaded()).toBe(false);
    // Mounted answers re-render when the table arrives, and are rendered again here.
    await loadTable();
    for (const streaming of [true, false]) surface(streaming).render(casesNode(CASES, streaming));
  }, 30_000);

  test.each([true, false])(
    "every case passes G4's DOM checks (streaming %s)",
    (streaming) => {
      const failures: string[] = [];
      for (const c of CASES) {
        const el = section(streaming, c.id);
        failures.push(...failuresIn(c.id, el));
        const text = el.textContent ?? "";
        // While streaming, the case is the last block: what must show then is `streamingText`, when given.
        for (const literal of (streaming ? c.streamingText : undefined) ?? c.text ?? [])
          if (!text.includes(literal))
            failures.push(`${c.id}: raw HTML not shown as text ("${literal}")`);
        for (const literal of streaming ? [] : (c.absent ?? []))
          if (text.includes(literal)) failures.push(`${c.id}: "${literal}" shows, but is markup`);
        for (const [selector, count] of Object.entries(c.tags ?? {})) {
          const found = el.querySelectorAll(selector).length;
          if (!streaming && found !== count)
            failures.push(`${c.id}: ${found} ${selector}, expected ${count}`);
        }
        const images = el.querySelectorAll("img").length;
        if (images !== (c.images ?? 0))
          failures.push(`${c.id}: ${images} <img>, expected ${c.images ?? 0}`);
        // The hint goes with a live link; while streaming, the case is the last block and its links are inert.
        if (
          !streaming &&
          c.linkHost &&
          !(el.querySelector('[data-clauseo="link-host"]')?.textContent ?? "").includes(c.linkHost)
        ) {
          failures.push(`${c.id}: real host ${c.linkHost} not shown next to the link text`);
        }
      }
      if (g4.__g4clobber !== undefined || document.getElementById("__g4clobber"))
        failures.push("dom-clobber: __g4clobber reachable");
      expect(failures).toEqual([]);
      expect(pwned).toEqual([]);
    },
    // About 2 s alone; under load past the default 5 s, so it gets the per-case test's 30 s.
    30_000,
  );

  test("every link click and middle click is cancelled; allowed URLs go to the host, nothing else does", () => {
    opened.length = 0;
    const links = [...surface(false).el.querySelectorAll("a")];
    let uncancelled = 0;
    for (const a of links) {
      for (const [type, button] of [
        ["click", 0],
        ["auxclick", 1],
      ] as const) {
        const event = new MouseEvent(type, { bubbles: true, cancelable: true, button });
        act(() => {
          a.dispatchEvent(event);
        });
        if (!event.defaultPrevented) uncancelled++;
      }
    }
    const openable = links.filter((a) =>
      /^(?:https?:|mailto:)/i.test(a.getAttribute("href") ?? ""),
    );
    expect(links.length).toBeGreaterThan(10);
    expect(uncancelled).toBe(0);
    expect(opened).toEqual(
      openable.flatMap((a) => [a.getAttribute("href"), a.getAttribute("href")]),
    );
    expect(opened.some((url) => /javascript|vbscript|data:|file:/i.test(url))).toBe(false);
  }, 30_000);

  test("the link-host hint follows the link's target in this state", () => {
    const el = section(false, "entity-sol-host");
    const href = el.querySelector("a")?.getAttribute("href") ?? "";
    const hint = el.querySelector('[data-clauseo="link-host"]')?.textContent ?? "";
    if (state === "loaded") {
      expect(new URL(href).hostname).toBe("evil.example");
      expect(hint).toContain("evil.example");
    } else {
      expect(new URL(href).hostname).toBe("bank.example");
      expect(hint).toBe("");
    }
  }, 30_000);

  if (state === "loaded") {
    test("a case without an `&` renders the same HTML, finished and streaming, as before the table loaded", () => {
      const differ: string[] = [];
      for (const streaming of [true, false]) {
        fresh().render(casesNode(CASES, streaming));
        for (const c of CASES.filter((c) => !hasReference(c))) {
          const key = `${c.id} (streaming ${streaming})`;
          if (html(section(streaming, c.id, fresh())) !== before.get(key)) differ.push(key);
        }
      }
      expect(before.size).toBe(2 * CASES.filter((c) => !hasReference(c)).length);
      expect(differ).toEqual([]);
    }, 30_000);

    test.each(CASES.filter(hasReference).map((c) => [c.id, c] as const))(
      "%s streamed in 16-character deltas stays clean at every step",
      (_, c) => {
        expect(streamFailures(stream(), c)).toEqual([]);
        expect(pwned).toEqual([]);
      },
      30_000,
    );
  }
});
