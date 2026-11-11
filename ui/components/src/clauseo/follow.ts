// Following the thread's end, as every chat app does. While the lawyer is at the end (within `NEAR` px of the bottom of
// what shows above the composer), whatever comes in, a new entry or more words in the answer that streams, keeps the
// view at the end. A move up by the lawyer (the wheel, a key, the scrollbar, a touch) lets go; so does anything else
// that scrolls the thread up, such as Find's jump to an entry. Content growing never does: it only ever adds below.
// Coming back down to the end follows again, and so does Latest (`end`) and a question sent.
//
// The place is read and set through `FollowPlace`, so the logic knows nothing of which element scrolls: the thread
// itself on a wide window, the page under 1000 px (thread.tsx gives both). Nothing here runs a timer or a frame of its
// own: it acts when the thread tells it something happened, once a frame at most for a scroll, and after layout for a
// change of size (a ResizeObserver), so a streamed line costs a few reads of what layout has already worked out and,
// at most, one write.

/** How close to the end, in px, still counts as the end. */
export const NEAR = 64;
/** The longest a glide to the end may take before it lands at once: the end moves while the answer streams. */
export const GLIDE_MS = 1000;

/** Where the thread's end is, and how to go there. */
export type FollowPlace = {
  /** Px from the end of the thread to the bottom of what shows: 0 at the end, more above it, less past it (the page
   * under 1000 px can scroll on past the thread, into the papers). */
  gap(): number;
  /** The scroll position, which grows going down: only its changes are read, to tell an upward move. */
  top(): number;
  /** Goes to the end: at once, or gliding. */
  toEnd(smooth: boolean): void;
};

export type Follow = {
  /** Whether the view keeps to the end as content comes in. */
  readonly following: boolean;
  /** Whether Latest shows: not following, with the end out of view. */
  readonly shown: boolean;
  /** The content or the view changed size: back to the end while following. */
  grew(): void;
  /** The view scrolled (once a frame): following again at the end, letting go on a move up. */
  scrolled(): void;
  /** The lawyer moved up (the wheel, a key, the scrollbar, a touch): let go at once, before the scroll lands, so a
   * line streaming in meanwhile doesn't pull the view back down. */
  leave(): void;
  /** Latest, or a question sent: to the end, following again. */
  end(smooth: boolean): void;
  /** Find's jump to an entry is landing: nothing follows while it does, and following stays off once it has. */
  hold(on: boolean): void;
};

/** The follow logic over `place`; `onShown` hears each change of whether Latest shows. */
export function createFollow(
  place: FollowPlace,
  onShown: (shown: boolean) => void,
  now: () => number = () => performance.now(),
): Follow {
  let following = true;
  let held = false;
  /** Gliding to the end since then, or null. */
  let glide: number | null = null;
  /** The scroll position when last read. */
  let last = 0;
  let shown = false;

  const atEnd = (gap: number) => gap <= NEAR && gap >= -NEAR;
  const show = () => {
    const next = !following && glide === null && place.gap() > 1;
    if (next === shown) return;
    shown = next;
    onShown(next);
  };
  /** Lands a glide that has taken too long, the end having moved on under it. */
  const late = () => {
    if (glide === null || now() - glide < GLIDE_MS) return false;
    glide = null;
    following = true;
    place.toEnd(false);
    last = place.top();
    return true;
  };

  return {
    get following() {
      return following;
    },
    get shown() {
      return shown;
    },
    grew() {
      const top = place.top();
      // something else moved the view up since it was last read (Find's scrollIntoView, in the same frame as rows
      // coming in): that lets go, as a scroll would once its event came
      if (glide === null && top < last - 1 && place.gap() > 1) following = false;
      last = top;
      if (held) return show();
      if (glide !== null) {
        // the end moved under the glide: aim again, or land if it's taking too long
        if (!late()) place.toEnd(true);
      } else if (following && Math.abs(place.gap()) >= 1) {
        place.toEnd(false);
        last = place.top();
      }
      show();
    },
    scrolled() {
      const top = place.top();
      const went = top - last;
      last = top;
      const gap = place.gap();
      if (glide !== null) {
        if (Math.abs(gap) <= 2) {
          glide = null;
          following = true;
        } else late();
      } else if (!held) {
        if (went < -1 && gap > 1) following = false;
        else if (atEnd(gap)) following = true;
      }
      show();
    },
    leave() {
      // only where there is somewhere to go up to: a wheel over a thread too short to scroll lets nothing go
      if (place.top() <= 0) return;
      following = false;
      glide = null;
      show();
    },
    end(smooth) {
      following = true;
      // already there, a glide would wait for a scroll that never comes
      glide = smooth && Math.abs(place.gap()) > 2 ? now() : null;
      place.toEnd(glide !== null);
      last = place.top();
      show();
    },
    hold(on) {
      held = on;
      if (on) {
        following = false;
        glide = null;
      }
      show();
    },
  };
}
