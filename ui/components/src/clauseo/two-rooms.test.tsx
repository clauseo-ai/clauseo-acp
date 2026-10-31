// @vitest-environment happy-dom
// Two rooms on one page, under StrictMode, so every effect runs, cleans up and runs again. Each room, built from the
// room's parts inside its own RoomScopeProvider, keeps its own ids, its own hand and its own accept: a press in one
// never reaches the other, and one room leaving the page leaves the other working.
import { act, StrictMode, useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { afterAll, beforeAll, expect, test, vi } from "vitest";
import { TooltipProvider } from "../ui/tooltip.tsx";
import { DEFAULT_DISPLAY, showDisplay } from "../lib/display.ts";
import { Accept } from "./accept.tsx";
import { AnswerBlocks } from "./answer.tsx";
import { Citations } from "./citations.tsx";
import { ExcerptCard } from "./excerpt-card.tsx";
import type { AcceptState } from "./logic.ts";
import type { MatterData } from "./matter.ts";
import { MatterContext } from "./matter-context.ts";
import { PapersWall } from "./paper-tile.tsx";
import { RoomScopeProvider } from "./room-scope-provider.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const CHECK = {
  waitingFor: "judgments since 2001 that cite it",
  result: "Good law",
  note: "Followed since.",
  ms: 900,
};
const MATTER: MatterData = {
  papers: [
    {
      id: "notice",
      title: "Notice to the bank.docx",
      pb: { from: 1, to: 4 },
      kind: "correspondence",
    },
    { id: "reply", title: "Reply to the notice", pb: { from: 5, to: 5 }, kind: "correspondence" },
    {
      id: "minutes",
      title: "Board minutes, 2 March 2001",
      pb: { from: 6, to: 9 },
      kind: "document",
    },
  ],
  citations: {
    answer: [
      {
        id: "notice-2",
        paperId: "notice",
        role: "supports",
        pin: "Para 2, PB 1",
        excerpt: "The bank was told in writing on the first of the month.",
        mark: "told in writing",
      },
      {
        id: "reply-3",
        paperId: "reply",
        role: "against",
        pin: "Para 3, PB 5",
        excerpt: "The bank denies that any notice reached it.",
        mark: "denies",
      },
    ],
  },
  inView: "answer",
  cites: {
    acme: {
      short: "Acme Ltd",
      name: "Acme Ltd v. State",
      cite: "(2001) 1 ABC 1",
      decided: "1 January 2001",
      court: "Supreme Court",
      forum: "court",
      bench: 3,
      pin: "para 4",
      stance: "for",
      check: CHECK,
    },
  },
};
const ACCEPT_WORDS = {
  action: "Accept into the matter",
  pending: "Not yet accepted",
  done: "Accepted",
  undo: "Undo",
};
const DRAFT = {
  id: "draft",
  short: "Draft reply",
  title: "Draft reply to the notice",
  label: "Prepared",
};

function Room() {
  const [accept, setAccept] = useState<AcceptState>("pending");
  const cite = MATTER.cites["acme"];
  return (
    <RoomScopeProvider className="room-window">
      <MatterContext value={MATTER}>
        <PapersWall papers={MATTER.papers} draft={DRAFT} onTurn={() => {}} />
        {cite !== undefined && <Citations cites={[{ id: "acme", cite }]} />}
        <AnswerBlocks
          answer="answer"
          blocks={[
            { kind: "lead", text: "The bank had notice [[p:notice-2|PB 1]], as [[c:acme]] holds." },
          ]}
          count="1"
        />
        <Accept
          state={accept}
          words={ACCEPT_WORDS}
          onAccept={() => setAccept("accepted")}
          onUndo={() => setAccept("pending")}
        />
        <ExcerptCard />
      </MatterContext>
    </RoomScopeProvider>
  );
}

let host: HTMLDivElement;
let root: ReturnType<typeof createRoot>;
const control = { showFirst: (_on: boolean) => {} };

function TwoRooms() {
  const [first, setFirst] = useState(true);
  useEffect(() => {
    control.showFirst = setFirst;
  }, []);
  return (
    <>
      <div data-test="a">{first && <Room />}</div>
      <div data-test="b">
        <Room />
      </div>
    </>
  );
}

const roomIn = (name: string) => {
  const box = document.querySelector(`[data-test="${name}"]`);
  if (box === null) throw new Error(`no room ${name}`);
  return box;
};
const one = <T extends Element>(box: Element, selector: string) => {
  const n = box.querySelector<T>(selector);
  if (n === null) throw new Error(`no ${selector}`);
  return n;
};
/** Lets every timer the parts set run out, inside act: no waiting on the clock. */
const settle = () =>
  act(async () => {
    vi.runAllTimers();
  });
const press = async (n: HTMLElement) => {
  await act(async () => n.click());
  await settle();
};

beforeAll(async () => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
  // motion decorates what lands; the test reads what lands
  showDisplay({ ...DEFAULT_DISPLAY, motion: "reduced" });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () =>
    root.render(
      <StrictMode>
        <TooltipProvider>
          <TwoRooms />
        </TooltipProvider>
      </StrictMode>,
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

test("both rooms stand whole, with no id on the page twice", () => {
  for (const name of ["a", "b"]) {
    const box = roomIn(name);
    expect(box.querySelectorAll(".room-window")).toHaveLength(1);
    expect(box.querySelectorAll(".tile")).toHaveLength(4);
    expect(box.querySelectorAll(".cite")).toHaveLength(1);
  }
  const ids = [...document.querySelectorAll("[id]")].map((n) => n.id);
  expect(ids.length).toBeGreaterThan(4);
  expect(ids.filter((id, i) => ids.indexOf(id) !== i)).toEqual([]);
  // every reference to an id finds it in its own room
  for (const name of ["a", "b"]) {
    const box = roomIn(name);
    for (const n of box.querySelectorAll(
      "[aria-labelledby], [aria-describedby], [aria-controls], a[href^='#']",
    )) {
      const refs = ["aria-labelledby", "aria-describedby", "aria-controls"].flatMap(
        (a) => n.getAttribute(a)?.split(" ") ?? [],
      );
      const href = n.getAttribute("href");
      if (href?.startsWith("#") === true) refs.push(href.slice(1));
      for (const id of refs) {
        const target = document.getElementById(id);
        expect(target, id).not.toBeNull();
        expect(box.contains(target)).toBe(true);
      }
    }
  }
});

test("a judgment's reference takes its short name from the matter in reach", () => {
  const refs = [...roomIn("a").querySelectorAll(".lead .ref")].map((n) => n.textContent);
  expect(refs).toEqual(["PB\u00A01", "Acme Ltd"]);
  expect(one(roomIn("a"), '.lead .ref[data-card="paper:notice"]').getAttribute("data-role")).toBe(
    "supports",
  );
});

test("a paper put in hand in one room lifts only that room's tile", async () => {
  const a = roomIn("a");
  const b = roomIn("b");
  const faceA = one<HTMLButtonElement>(a, '.tile-face[data-card="paper:reply"]');
  await press(faceA);
  const tileA = faceA.parentElement;
  const tileB = one<HTMLElement>(b, '.tile-face[data-card="paper:reply"]').parentElement;
  expect(tileA?.hasAttribute("data-hand")).toBe(true);
  expect(tileB?.hasAttribute("data-hand")).toBe(false);
  expect(document.querySelectorAll(".card")).toHaveLength(1);
  expect(one(document.body, ".card").getAttribute("data-role")).toBe("against");
  expect(one(document.body, ".card").textContent).toContain("Reply to the notice");
  // the same press puts it down
  await press(faceA);
  expect(tileA?.hasAttribute("data-hand")).toBe(false);
  expect(document.querySelectorAll(".card")).toHaveLength(0);
});

test("an uncited paper has no card to put in hand", () => {
  const face = one<HTMLButtonElement>(roomIn("a"), '.tile[data-state="uncited"] .tile-face');
  expect(face.hasAttribute("data-card")).toBe(false);
});

test("accepting in one room leaves the other's answer as it was", async () => {
  const a = roomIn("a");
  const b = roomIn("b");
  await press(one<HTMLButtonElement>(a, ".accept-btn"));
  expect(one(a, ".accept-row").getAttribute("data-state")).toBe("accepted");
  expect(one(b, ".accept-row").getAttribute("data-state")).toBe("pending");
});

test("one room leaving the page leaves the other working", async () => {
  await act(async () => control.showFirst(false));
  await settle();
  expect(roomIn("a").querySelector(".room-window")).toBeNull();
  const b = roomIn("b");
  const face = one<HTMLButtonElement>(b, '.tile-face[data-card="paper:notice"]');
  await press(face);
  expect(face.parentElement?.hasAttribute("data-hand")).toBe(true);
  expect(document.querySelectorAll(".card")).toHaveLength(1);
  expect(one(document.body, ".card").getAttribute("data-role")).toBe("supports");
  await press(one<HTMLButtonElement>(b, ".accept-btn"));
  expect(one(b, ".accept-row").getAttribute("data-state")).toBe("accepted");
});
