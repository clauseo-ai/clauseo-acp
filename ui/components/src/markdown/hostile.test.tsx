// @vitest-environment happy-dom
// The hostile cases (G4's 28 and the ones added since), rendered by the renderer in happy-dom, with every
// check of G4's conformance page that a DOM can show: no forbidden element, links and images only through
// our components, allowed protocols only, every id prefixed, no event-handler attribute, raw HTML visible
// as text, the link-host hint, no clobbered global, every link click cancelled and handed to the host.
// Each case is also streamed in 16-character deltas, checked after every delta. What needs a real engine
// (CSP and Trusted Types violations, a trusted click not navigating, network requests, a payload running)
// is the private harness's, which imports hostile-cases.ts too. Everything runs twice: before the full
// entity table has loaded (its prefetch is held back here) and after (entities.ts), since a character
// reference outside the common set decodes only in the second state.
import { act } from "react";
import { createRoot } from "react-dom/client";
import type { Root } from "react-dom/client";
import { afterAll, beforeAll, describe, expect, test, vi } from "vitest";
import { characterEntitiesLoaded, loadEntities } from "./entities.ts";
import { CASES } from "./hostile-cases.ts";
import { MarkdownHostContext } from "./host.ts";
import type { MarkdownHost } from "./host.ts";
import { Markdown } from "./markdown.tsx";

// The component's idle prefetch would load the table part-way through the first state.
vi.mock("./entities.ts", async (original) => ({
  ...(await original<typeof import("./entities.ts")>()),
  prefetchEntities: () => {},
}));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const FORBIDDEN =
  "script,iframe,frame,object,embed,style,link,meta,form,input,noscript,base,template,textarea,svg,math,button";
const LINK_OK = /^(?:https?:|mailto:|#)/i;
const IMAGE_OK = /^(?:clauseo-file:|data:image\/(?:png|jpeg|gif|webp);base64,)/i;
const DELTA = 16;

/** Every failure G4's checkContainer would report for one container. */
function failuresIn(label: string, el: Element): string[] {
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
  for (const node of el.querySelectorAll("*")) {
    for (const attribute of node.getAttributeNames()) {
      if (/^on/i.test(attribute)) failures.push(`${label}: event handler attribute ${attribute}`);
    }
  }
  return failures;
}

const opened: string[] = [];
const references: string[] = [];
const HOST: MarkdownHost = {
  openExternal: (url) => opened.push(url),
  openReference: (kind, id) => references.push(`${kind}:${id}`),
};
const pwned: string[] = [];
const g4 = globalThis as { __g4pwn?: (id: string) => void; __g4clobber?: unknown };

let host: HTMLDivElement;
let root: Root;

beforeAll(() => {
  g4.__g4pwn = (id) => pwned.push(id);
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterAll(() => {
  act(() => root.unmount());
  host.remove();
  delete g4.__g4pwn;
});

function renderCases(streaming: boolean) {
  act(() =>
    root.render(
      <MarkdownHostContext value={HOST}>
        {CASES.map((c) => (
          <section key={c.id} data-case={c.id}>
            <Markdown text={c.markdown} streaming={streaming} />
          </section>
        ))}
      </MarkdownHostContext>,
    ),
  );
}

test("the corpus holds G4's 28 cases and the ones added since, each with its own id", () => {
  expect(CASES[27]?.id).toBe("unterminated-fence");
  expect(CASES).toHaveLength(61);
  expect(new Set(CASES.map((c) => c.id)).size).toBe(CASES.length);
});

describe.each(["not loaded", "loaded"])("entity table %s", (state) => {
  beforeAll(async () => {
    // Mounted answers re-render when the table arrives.
    if (state === "loaded") await act(() => loadEntities());
    expect(characterEntitiesLoaded()).toBe(state === "loaded");
  });

  test.each([true, false])("every case passes G4's DOM checks (streaming %s)", (streaming) => {
    renderCases(streaming);
    const failures: string[] = [];
    for (const c of CASES) {
      const el = host.querySelector(`[data-case="${c.id}"]`);
      if (!el) throw new Error(`${c.id} not rendered`);
      failures.push(...failuresIn(c.id, el));
      const text = el.textContent ?? "";
      for (const literal of c.text ?? [])
        if (!text.includes(literal))
          failures.push(`${c.id}: raw HTML not shown as text ("${literal}")`);
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
  });

  test("every link click and middle click is cancelled; allowed URLs go to the host, nothing else does", () => {
    renderCases(false);
    opened.length = 0;
    const links = [...host.querySelectorAll("a")];
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
  });

  /**
   * The prefix lengths a case streams through, 16 characters apart. The 20,000-character line is one paragraph
   * that is parsed again on every delta (tens of milliseconds a delta near its end here, about a minute for all
   * 1,250 deltas); it streams its first 1,000 and its last 320 characters (its middle repeats the same 67 characters).
   */
  function ends(length: number): number[] {
    const all: number[] = [];
    for (let end = DELTA; end < length + DELTA; end += DELTA) all.push(Math.min(end, length));
    return length <= 2_000 ? all : all.filter((end) => end <= 1_000 || end >= length - 320);
  }

  test.each(CASES.map((c) => [c.id, c] as const))(
    "%s streamed in 16-character deltas stays clean at every step",
    (_, c) => {
      const failures: string[] = [];
      for (const end of ends(c.markdown.length)) {
        act(() =>
          root.render(
            <MarkdownHostContext value={HOST}>
              <section data-case={c.id}>
                <Markdown text={c.markdown.slice(0, end)} streaming />
              </section>
            </MarkdownHostContext>,
          ),
        );
        failures.push(...failuresIn(`${c.id} at ${end}`, host));
        // Text that has fully arrived shows, mid-stream too: nothing half-arrived hides what follows it.
        const shown = host.textContent ?? "";
        for (const literal of c.streamingText ?? c.text ?? []) {
          if (c.markdown.slice(0, end).includes(literal) && !shown.includes(literal))
            failures.push(`${c.id} at ${end}: "${literal}" arrived but does not show`);
        }
      }
      act(() => root.render(null));
      expect([...new Set(failures)]).toEqual([]);
      expect(pwned).toEqual([]);
    },
    30_000,
  );

  test("the link-host hint follows the link's target in this state", () => {
    renderCases(false);
    const el = host.querySelector('[data-case="entity-sol-host"]')!;
    const href = el.querySelector("a")?.getAttribute("href") ?? "";
    const hint = el.querySelector('[data-clauseo="link-host"]')?.textContent ?? "";
    if (state === "loaded") {
      expect(new URL(href).hostname).toBe("evil.example");
      expect(hint).toContain("evil.example");
    } else {
      expect(new URL(href).hostname).toBe("bank.example");
      expect(hint).toBe("");
    }
  });
});
