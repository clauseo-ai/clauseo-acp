// The follow logic (follow.ts) over a scroller of its own: a content height, a view height and a scroll position the
// browser would clamp. Growth at the end keeps the view there; a move up lets go and shows Latest; Latest and a
// question sent go back and follow; Find's jump and older pages going in above never pull the view to the end.
import { expect, test } from "vitest";
import { createFollow, GLIDE_MS, NEAR, type FollowPlace } from "./follow.ts";

/** A scroller: `height` of content in a `view` px high window, scrolled to `y`; `glides` counts the smooth goes. */
function scroller(height: number, view = 500) {
  const s = {
    height,
    view,
    y: 0,
    glides: 0,
    jumps: 0,
    set(y: number) {
      s.y = Math.max(0, Math.min(y, s.height - s.view));
    },
  };
  const place: FollowPlace = {
    gap: () => s.height - s.y - s.view,
    top: () => s.y,
    toEnd: (smooth) => {
      if (smooth) s.glides += 1;
      else {
        s.jumps += 1;
        s.set(s.height);
      }
    },
  };
  return { s, place };
}

function setup(height = 2000, view = 500) {
  const { s, place } = scroller(height, view);
  const shown: boolean[] = [];
  let clock = 0;
  const follow = createFollow(
    place,
    (v) => shown.push(v),
    () => clock,
  );
  return {
    s,
    follow,
    shown,
    tick: (ms: number) => (clock += ms),
    /** The lawyer, or anything else, scrolls to `y`; its scroll event arrives. */
    scroll: (y: number) => {
      s.set(y);
      follow.scrolled();
    },
    /** `by` px more content at the end; the observer reports it after layout. */
    grow: (by: number) => {
      s.height += by;
      follow.grew();
    },
  };
}

const atEnd = (s: { height: number; y: number; view: number }) => s.y === s.height - s.view;

test("at the end, growth keeps the view at the end, a streamed line at a time", () => {
  const t = setup();
  t.follow.grew();
  expect(atEnd(t.s)).toBe(true);
  for (let i = 0; i < 200; i += 1) {
    t.grow(3);
    expect(atEnd(t.s)).toBe(true);
    // the scroll the follow set comes back as an event, at the end: still following
    t.follow.scrolled();
  }
  expect(t.follow.following).toBe(true);
  expect(t.follow.shown).toBe(false);
  expect(t.shown).toEqual([]);
});

test("within NEAR px of the end counts as the end; a move down to it follows again", () => {
  const t = setup();
  t.follow.grew();
  t.scroll(t.s.y - 300);
  expect(t.follow.following).toBe(false);
  t.scroll(t.s.height - t.s.view - (NEAR - 4));
  expect(t.follow.following).toBe(true);
  t.grow(40);
  expect(atEnd(t.s)).toBe(true);
});

test("after the lawyer scrolls up, growth leaves the view where it is and Latest shows", () => {
  const t = setup();
  t.follow.grew();
  // the wheel lets go first, before its scroll lands
  t.follow.leave();
  t.scroll(t.s.y - 400);
  const y = t.s.y;
  expect(t.follow.following).toBe(false);
  expect(t.follow.shown).toBe(true);
  for (let i = 0; i < 50; i += 1) t.grow(20);
  expect(t.s.y).toBe(y);
  expect(t.follow.shown).toBe(true);
  expect(t.shown).toEqual([true]);
});

test("a wheel up lets go even before its scroll lands, so a line streaming in meanwhile doesn't pull the view down", () => {
  const t = setup();
  t.follow.grew();
  const y = t.s.y;
  t.follow.leave();
  t.grow(30);
  expect(t.s.y).toBe(y);
});

test("a move up on a thread too short to scroll lets nothing go", () => {
  const t = setup(300, 500);
  t.follow.grew();
  t.follow.leave();
  expect(t.follow.following).toBe(true);
  t.grow(900);
  expect(atEnd(t.s)).toBe(true);
});

test("Latest glides to the end, hides, and follows again; growth on the way aims again", () => {
  const t = setup();
  t.follow.grew();
  t.follow.leave();
  t.scroll(200);
  expect(t.follow.shown).toBe(true);
  t.follow.end(true);
  expect(t.s.glides).toBe(1);
  expect(t.follow.shown).toBe(false);
  expect(t.follow.following).toBe(true);
  // the glide's own scroll events, on their way down, never read as the lawyer leaving
  t.scroll(600);
  t.grow(20);
  expect(t.s.glides).toBe(2);
  t.scroll(1200);
  expect(t.follow.shown).toBe(false);
  t.scroll(t.s.height);
  expect(t.follow.following).toBe(true);
  // landed: growth jumps again, at once
  t.grow(20);
  expect(atEnd(t.s)).toBe(true);
  expect(t.shown).toEqual([true, false]);
});

test("a glide the end keeps running from lands at once after GLIDE_MS", () => {
  const t = setup();
  t.follow.grew();
  t.follow.leave();
  t.scroll(100);
  t.follow.end(true);
  t.scroll(400);
  t.tick(GLIDE_MS + 1);
  t.grow(10);
  expect(atEnd(t.s)).toBe(true);
  expect(t.follow.following).toBe(true);
});

test("a wheel up during Latest's glide stops it there", () => {
  const t = setup();
  t.follow.grew();
  t.follow.leave();
  t.scroll(100);
  t.follow.end(true);
  t.scroll(500);
  t.follow.leave();
  t.scroll(450);
  t.grow(50);
  expect(t.s.y).toBe(450);
  expect(t.follow.shown).toBe(true);
});

test("a question sent goes to the end at once and follows its answer", () => {
  const t = setup();
  t.follow.grew();
  t.follow.leave();
  t.scroll(0);
  t.grow(200);
  expect(t.s.y).toBe(0);
  t.follow.end(false);
  expect(atEnd(t.s)).toBe(true);
  expect(t.s.glides).toBe(0);
  expect(t.follow.shown).toBe(false);
  t.grow(80);
  expect(atEnd(t.s)).toBe(true);
});

test("Find's jump isn't followed away: held while it lands, and let go once it has", () => {
  const t = setup(5000);
  t.follow.grew();
  t.follow.hold(true);
  // rows page in above, and the found row is scrolled to the middle in the same frame
  t.s.height += 3000;
  t.s.set(1200);
  t.follow.grew();
  expect(t.s.y).toBe(1200);
  t.follow.scrolled();
  t.follow.hold(false);
  // the answer streams on below: the found entry stays put
  t.grow(40);
  t.follow.scrolled();
  t.grow(40);
  expect(t.s.y).toBe(1200);
  expect(t.follow.shown).toBe(true);
});

test("a jump up by anything else in the frame rows come in lets go before the observer can pull the view down", () => {
  const t = setup(5000);
  t.follow.grew();
  // no hold: scrollIntoView moved the view up, then the observer reports growth before the scroll event comes
  t.s.height += 100;
  t.s.set(800);
  t.follow.grew();
  expect(t.s.y).toBe(800);
  expect(t.follow.following).toBe(false);
});

test("older entries going in above never jump to the end: the rows keep their place", () => {
  const t = setup(3000);
  t.follow.grew();
  // the lawyer reads up to the top, where the next page is asked for
  t.follow.leave();
  t.scroll(0);
  // the page goes in above, and older.ts keeps the rows where they were by moving the scroll down as much
  t.s.height += 2500;
  t.s.set(t.s.y + 2500);
  t.follow.grew();
  t.follow.scrolled();
  expect(t.s.y).toBe(2500);
  expect(t.follow.following).toBe(false);
  expect(t.s.jumps).toBe(1);
});

test("content shrinking at the end (a fold closing) keeps the view following", () => {
  const t = setup();
  t.follow.grew();
  t.s.height -= 300;
  t.s.set(t.s.y);
  t.follow.scrolled();
  t.follow.grew();
  expect(t.follow.following).toBe(true);
  t.grow(10);
  expect(atEnd(t.s)).toBe(true);
});
