// The settings window follows its address's hash (U15): the section the page opens at, `#connectors` from the room's
// hint and banner, and each later change of hash, which the host makes when the window is open already
// (crates/desktop/src/settings_window.rs sets `location.hash`). A wanted section that isn't shown yet is held, not
// dropped, until the page is `ready` to say what shows (the first connectors state, since the Connectors section shows
// only once a state lists the connector): then it opens if it shows, and otherwise the first section opens, as
// sectionOf does. Each open a hash caused bumps `focusHead`, so the frame focuses the section's head; a press on the
// side writes the hash without an event (replaceState) and moves no focus.
import { useEffect, useRef, useState } from "react";

/** A hash as a section's id: "#connectors" is "connectors". */
export const hashId = (hash: string): string => hash.replace(/^#/, "");

type Follow = {
  /** The section chosen last, by the side or a hash; undefined opens the first shown. */
  chosen: string | undefined;
  /** The section a hash asked for that isn't shown yet. */
  wanted: string | null;
  /** How many opens a hash caused. */
  focus: number;
};

export type HashSection = {
  /** The open section's id: the chosen one while it shows, else the first shown. */
  open: string | undefined;
  /** Goes up by one each time a hash opened a section. */
  focusHead: number;
  /** A press on the side: opens `id` and keeps it in the address, so a reload opens it again. */
  choose: (id: string) => void;
};

const startOf = (want: string, shown: readonly string[], focus: boolean): Follow =>
  shown.includes(want)
    ? { chosen: want, wanted: null, focus: focus ? 1 : 0 }
    : { chosen: undefined, wanted: want === "" ? null : want, focus: 0 };

/**
 * The open section over `shown` (the shown sections' ids, in order). `fixed`, for the review page, opens that section
 * in place of the address's and follows no hash. `ready` says the shown list is whole: until then a wanted section
 * that isn't in it waits.
 */
export function useHashSection(
  shown: readonly string[],
  ready: boolean,
  fixed?: string,
): HashSection {
  const [follow, setFollow] = useState<Follow>(() =>
    fixed !== undefined
      ? startOf(fixed, shown, false)
      : startOf(hashId(globalThis.location?.hash ?? ""), shown, true),
  );
  // a held section opens once it shows, or gives way to the first once the page knows it won't: adjusted as this render
  // finds it, React's pattern for state that follows props, so no frame shows the old section
  const { wanted } = follow;
  if (wanted !== null && shown.includes(wanted))
    setFollow({
      chosen: wanted,
      wanted: null,
      focus: follow.focus + (fixed === undefined ? 1 : 0),
    });
  else if (wanted !== null && ready) setFollow({ ...follow, chosen: undefined, wanted: null });
  // the hash the side wrote last: a page that hears its own replaceState as a hashchange (happy-dom does) moves no
  // focus for it
  const written = useRef<string | null>(null);
  // a later hash, from the host
  useEffect(() => {
    if (fixed !== undefined) return;
    const onHash = () => {
      const id = hashId(globalThis.location.hash);
      if (id === written.current) {
        written.current = null;
        return;
      }
      written.current = null;
      if (id !== "") setFollow((f) => ({ ...f, wanted: id }));
    };
    globalThis.addEventListener("hashchange", onHash);
    return () => globalThis.removeEventListener("hashchange", onHash);
  }, [fixed]);
  const open = shown.find((id) => id === follow.chosen) ?? shown[0];
  return {
    open,
    focusHead: follow.focus,
    choose: (id) => {
      if (!shown.includes(id)) return;
      setFollow((f) => ({ ...f, chosen: id, wanted: null }));
      written.current = id;
      history.replaceState(history.state, "", `#${id}`);
    },
  };
}
