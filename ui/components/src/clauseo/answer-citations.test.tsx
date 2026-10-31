// @vitest-environment happy-dom
// Two answers in one room, each with its own citations. A reference names its own answer's citation by the
// citation's id, so two places quoted in one paper open their own cards, and a reference can't reach another
// answer's citation. The wall lights a paper for the answer in view. Invented data.
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterAll, beforeAll, expect, test, vi } from "vitest";
import { TooltipProvider } from "../ui/tooltip.tsx";
import { DEFAULT_DISPLAY, showDisplay } from "../lib/display.ts";
import { AnswerBlocks } from "./answer.tsx";
import { ExcerptCard } from "./excerpt-card.tsx";
import type { MatterData } from "./matter.ts";
import { MatterContext } from "./matter-context.ts";
import { PapersWall } from "./paper-tile.tsx";
import { RoomScopeProvider } from "./room-scope-provider.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const MATTER: MatterData = {
  papers: [
    { id: "notice", title: "Notice to the bank", pb: { from: 1, to: 4 }, kind: "correspondence" },
    { id: "reply", title: "Reply to the notice", pb: { from: 5, to: 5 }, kind: "correspondence" },
  ],
  cites: {},
  citations: {
    a: [
      {
        id: "n2",
        paperId: "notice",
        role: "supports",
        pin: "Para 2, PB 1",
        excerpt: "The bank was told in writing.",
        mark: "",
      },
      {
        id: "n9",
        paperId: "notice",
        role: "against",
        pin: "Para 9, PB 3",
        excerpt: "No copy went to the branch.",
        mark: "",
      },
    ],
    b: [
      {
        id: "r3",
        paperId: "reply",
        role: "against",
        pin: "Para 3, PB 5",
        excerpt: "The bank denies it.",
        mark: "",
      },
    ],
  },
  inView: "a",
};

let host: HTMLDivElement;
let root: ReturnType<typeof createRoot>;

const one = <T extends Element>(selector: string) => {
  const n = document.querySelector<T>(selector);
  if (n === null) throw new Error(`no ${selector}`);
  return n;
};
const ref = (answer: string, label: string) => {
  const n = [...document.querySelectorAll<HTMLAnchorElement>(`[data-test="${answer}"] .ref`)].find(
    (r) => r.textContent === label,
  );
  if (n === undefined) throw new Error(`no ${label} in ${answer}`);
  return n;
};
const settle = () =>
  act(async () => {
    vi.runAllTimers();
  });
const press = async (n: HTMLElement) => {
  await act(async () => n.click());
  await settle();
};
const cards = () => document.querySelectorAll(".card");

beforeAll(async () => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
  showDisplay({ ...DEFAULT_DISPLAY, motion: "reduced" });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () =>
    root.render(
      <TooltipProvider>
        <RoomScopeProvider className="room-window">
          <MatterContext value={MATTER}>
            <PapersWall papers={MATTER.papers} draft={null} onTurn={() => {}} />
            <div data-test="a">
              <AnswerBlocks
                answer="a"
                blocks={[{ kind: "lead", text: "[[p:n2|two]] [[p:n9|nine]] [[p:zz|none]]" }]}
                count="0"
              />
            </div>
            <div data-test="b">
              <AnswerBlocks
                answer="b"
                blocks={[{ kind: "lead", text: "[[p:n9|nine]] [[p:r3|three]]" }]}
                count="0"
              />
            </div>
            <ExcerptCard />
          </MatterContext>
        </RoomScopeProvider>
      </TooltipProvider>,
    ),
  );
  await settle();
});

afterAll(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.useRealTimers();
  showDisplay(DEFAULT_DISPLAY);
});

test("two citations of one paper open their own pin and excerpt", async () => {
  for (const [label, pin, excerpt, role] of [
    ["two", "Para\u00A02, PB\u00A01", "The bank was told in writing.", "supports"],
    ["nine", "Para\u00A09, PB\u00A03", "No copy went to the branch.", "against"],
  ] as const) {
    const r = ref("a", label);
    expect(r.getAttribute("data-card")).toBe("paper:notice");
    expect(r.getAttribute("data-role")).toBe(role);
    await press(r);
    expect(cards()).toHaveLength(1);
    expect(one(".card .card-pin").textContent).toBe(pin);
    expect(one(".card .card-quote").textContent).toBe(excerpt);
    expect(one(".card").getAttribute("data-role")).toBe(role);
    // the paper's tile is in hand, whichever of its citations opened the card
    expect(
      one('.tile-face[data-card="paper:notice"]').parentElement?.hasAttribute("data-hand"),
    ).toBe(true);
    await press(r);
    expect(cards()).toHaveLength(0);
  }
});

test("a reference can't reach another answer's citation, and an id no answer holds opens nothing", async () => {
  for (const r of [ref("b", "nine"), ref("a", "none")]) {
    expect(r.hasAttribute("data-card")).toBe(false);
    expect(r.hasAttribute("href")).toBe(false);
    await press(r);
    expect(cards()).toHaveLength(0);
  }
  // answer b's own citation still opens
  await press(ref("b", "three"));
  expect(one(".card .card-quote").textContent).toBe("The bank denies it.");
  await press(ref("b", "three"));
});

test("the wall lights a paper cited twice in the stronger role, and its tile opens that citation", async () => {
  const notice = one<HTMLButtonElement>('.tile-face[data-card="paper:notice"]');
  expect(notice.parentElement?.getAttribute("data-state")).toBe("against");
  // the reply is cited only by answer b, which isn't in view
  expect(document.querySelector('.tile-face[data-card="paper:reply"]')).toBeNull();
  await press(notice);
  expect(one(".card .card-pin").textContent).toBe("Para\u00A09, PB\u00A03");
  await press(notice);
  expect(cards()).toHaveLength(0);
});
