// The rail: the matters on the bare ground, grouped by their next date, with the current matter's conversations
// opened under its row, finding a matter, and the scratch matter at its foot. Matters, their states and their next
// dates come in as props.
import { useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { Button } from "../ui/button.tsx";
import { searchBox, searchInput } from "../ui/recipes.ts";
import { IconNewMatter, IconScratchMatter, IconSearch } from "../icons/icons.tsx";
import { cn } from "../lib/cn.ts";
import { useDisplay } from "../lib/display.ts";
import { useWords } from "../lib/words.ts";
import { frame } from "../lib/words-frame.ts";
import { mt } from "../lib/words-matters.ts";
import { ui } from "../lib/words-ui.ts";
import { useRoom } from "./room-scope.ts";
import { Say } from "./say.tsx";
import { Heading } from "./heading.tsx";
import { findMatters, type FoundBy } from "./rail-find.ts";
import { MatterRow, type MatterRowProps } from "./rail-matter.tsx";

export type { MatterRowProps, MatterState } from "./rail-matter.tsx";
export { MatterRow };

export type RailGroup = { when: string; matters: readonly MatterRowProps[] };

/** A matter the rail can find: its row, with its client and case number. */
export type FindableRow = MatterRowProps & { client: string; caseNo: string };

/** The list on first launch: what goes here and what Clauseo does with it. New matter, above, is the way forward. */
function NoMatters() {
  const w = useWords();
  return (
    <div className="grid gap-s1 px-s3 mt-s5">
      <p className="text-ui font-chip text-primary deva-nudge">{frame(w.lang, "No matters yet")}</p>
      <p className="text-small text-quiet">
        <Say>
          {frame(
            w.lang,
            "Add a matter's folder. Clauseo reads its papers before your first question.",
          )}
        </Say>
      </p>
    </div>
  );
}

/** What a search found, said where the name alone doesn't show why. */
function matchLine(lang: "en" | "hi", m: FindableRow, by: FoundBy): string | undefined {
  if (by === "client") return mt(lang, "Client, {client}", { client: m.client });
  if (by === "caseNo") return m.caseNo;
  return undefined;
}

/** Finding a matter: a field set into the ground under New matter. Focused and empty, the list turns to the matters
 * opened most recently; typed into, it turns to every matter whose name, client or case number holds the words. The
 * date groups come back when the field is left empty, or Escape clears it. */
export type RailFind = {
  all: readonly FindableRow[];
  /** The matters opened most recently, newest first. Their rows show no match line: the order is all that says when
   * each was opened. */
  recent: readonly MatterRowProps[];
  /** Where the field starts: words in it, or focused and empty. */
  query?: string;
  open?: boolean;
};

function FindField({
  query,
  onQuery,
  onOpen,
  onClose,
  onFirst,
}: {
  query: string;
  onQuery: (q: string) => void;
  onOpen: () => void;
  onClose: () => void;
  onFirst: () => void;
}) {
  const w = useWords();
  const room = useRoom();
  const label = mt(w.lang, "Find a matter by its name, client or case number");
  return (
    <label className={cn(searchBox, "w-full min-w-0 room-find")}>
      <IconSearch size={16} />
      <span className="sr-only">{label}</span>
      <input
        id={room.id("find")}
        type="search"
        autoComplete="off"
        className={searchInput}
        placeholder={mt(w.lang, "Find a matter")}
        value={query}
        onChange={(e) => onQuery(e.target.value)}
        onFocus={onOpen}
        onBlur={onClose}
        onKeyDown={(e) => {
          if (e.key === "Escape") {
            onQuery("");
            e.currentTarget.blur();
          } else if (e.key === "Enter") {
            e.preventDefault();
            onFirst();
          }
        }}
      />
    </label>
  );
}

/** The rail: matters on the bare ground, grouped by their next date, under a full-width New matter, with Scratch
 * matter, "Quick questions", below a hairline. Pinned matters stand above every date, under "Pinned". With no
 * matters at all, the rail says so. Given find, a field under New matter finds any matter; given settings, the
 * rail's foot links to them. */
export function Rail({
  groups,
  pinned = [],
  open,
  onNewMatter,
  find,
  scratch,
  settings,
  notice,
  label,
}: {
  groups: readonly RailGroup[];
  pinned?: readonly MatterRowProps[];
  open: boolean;
  /** New matter, pressed. */
  onNewMatter?: () => void;
  find?: RailFind;
  /** The scratch matter: whether it's the room, and its row pressed. */
  scratch?: { current?: boolean; onOpen?: () => void };
  /** Where Settings lives, linked from the rail's foot. */
  settings?: string;
  /** Optional: what sets this rail apart where several stand on one page, added to its name, "Matters, …". */
  label?: string;
  /** Optional: what the room says about Clauseo itself (an update, the first crash, a lab's terms),
   * as a RailNotice, in its own row at the rail's foot above Scratch matter. */
  notice?: ReactNode;
}) {
  const w = useWords();
  const room = useRoom();
  const { platform } = useDisplay();
  const [query, setQuery] = useState(find?.query ?? "");
  const [finding, setFinding] = useState(find?.open === true || (find?.query ?? "") !== "");
  // a press inside the rail keeps the list as it is until the press lands, so a found matter can be opened
  const pressing = useRef(false);
  const nav = useRef<HTMLElement>(null);
  const found = find === undefined ? [] : findMatters(find.all, query);
  const here =
    [...pinned, ...groups.flatMap((g) => g.matters)].find((m) => m.current === true)?.name ?? null;
  // the matter in view stands in view on the rail, its conversations under it: below the sticky head, at a row's top,
  // so the list never stops mid-row; on a mid window, each time the rail opens over the work
  useLayoutEffect(() => {
    const rail = nav.current;
    if (rail === null || finding || rail.clientHeight === 0) return;
    const row = rail.querySelector<HTMLElement>(
      ".room-groups .room-matter-row[aria-current='page']",
    );
    if (row === null) return;
    const house = row.closest<HTMLElement>(".room-house") ?? row;
    const head = rail.querySelector<HTMLElement>(".room-rail-head")?.offsetHeight ?? 0;
    const foot = rail.querySelector<HTMLElement>(".room-rail-foot")?.offsetHeight ?? 0;
    const section = house.closest("section");
    // the group's heading comes along when it fits, so the matter keeps its date group in view
    const lead =
      section !== null && section.querySelector(".room-matters > li") === house.closest("li")
        ? (section.querySelector<HTMLElement>(".room-when")?.offsetHeight ?? 0)
        : 0;
    const base = rail.getBoundingClientRect().top - rail.scrollTop;
    const top = house.getBoundingClientRect().top - base;
    const view = rail.clientHeight - head - foot;
    const highest = top - head - lead;
    if (highest >= rail.scrollTop && top + house.offsetHeight <= rail.scrollTop + head + view)
      return;
    // the least scroll that shows the whole matter, then up to the nearest top of a row or a date group, so the list
    // under the head never starts mid-row
    const least = Math.max(0, top + house.offsetHeight - head - view);
    const marks = [
      ...rail.querySelectorAll<HTMLElement>(".room-groups > section, .room-matters > li"),
    ]
      .map((n) => n.getBoundingClientRect().top - base - head)
      .filter((y) => y >= least && y <= highest && y <= rail.scrollHeight - rail.clientHeight)
      .sort((x, y) => x - y);
    rail.scrollTop = Math.max(0, marks[0] ?? highest);
  }, [here, open, finding]);
  const hasNotice = notice !== undefined;
  // the head and the foot cast an edge while the list runs under them, so a row passing under reads as passing under
  // an edge, never as a row cut in half. Read on scroll and when the rail or its list changes size, so nothing runs at
  // rest.
  useLayoutEffect(() => {
    const rail = nav.current;
    if (rail === null) return;
    const mark = () => {
      rail
        .querySelector(":scope > .room-rail-head")
        ?.toggleAttribute("data-stuck", rail.scrollTop > 0);
      rail
        .querySelector(":scope > .room-rail-foot")
        ?.toggleAttribute("data-under", rail.scrollTop + rail.clientHeight < rail.scrollHeight - 1);
    };
    mark();
    rail.addEventListener("scroll", mark, { passive: true });
    const sizes = typeof ResizeObserver === "function" ? new ResizeObserver(mark) : null;
    sizes?.observe(rail);
    for (const child of rail.children) sizes?.observe(child);
    return () => {
      rail.removeEventListener("scroll", mark);
      sizes?.disconnect();
    };
  }, [finding, query, groups, pinned, hasNotice]);
  const leave = () => {
    setQuery("");
    setFinding(false);
  };
  const all: readonly RailGroup[] = [
    ...(pinned.length > 0 ? [{ when: frame(w.lang, "Pinned"), matters: pinned }] : []),
    ...groups.filter((g) => g.matters.length > 0),
  ];
  const opening = (m: MatterRowProps): MatterRowProps => ({
    ...m,
    onOpen: () => {
      leave();
      m.onOpen?.();
    },
  });
  const shown: readonly RailGroup[] | "none" =
    find === undefined || !finding
      ? all
      : query.trim() === ""
        ? [{ when: mt(w.lang, "Recent"), matters: find.recent.map(opening) }]
        : found.length === 0
          ? "none"
          : [
              {
                when: mt(w.lang, "Matching matters"),
                matters: found.map((f) =>
                  opening({
                    ...f.matter,
                    current: false,
                    conversations: undefined,
                    match: matchLine(w.lang, f.matter, f.by),
                  }),
                ),
              },
            ];
  const scratchRow = (
    <div className="room-scratch">
      <button
        type="button"
        className="room-matter-row"
        aria-current={scratch?.current === true ? "page" : undefined}
        onClick={scratch?.onOpen}
      >
        <IconScratchMatter size={16} className="room-scratch-icon" />
        <span className="room-scratch-text">
          <span className="room-matter-name">{w.t("Scratch matter")}</span>
          <span className="room-matter-note">{w.t("Quick questions")}</span>
        </span>
      </button>
    </div>
  );
  return (
    <nav
      ref={nav}
      className="room-rail"
      id={room.id("rail")}
      aria-label={label === undefined ? w.t("Matters") : `${w.t("Matters")}, ${label}`}
      data-open={open ? "" : undefined}
      onPointerDownCapture={() => {
        pressing.current = true;
      }}
      onPointerUp={() => {
        pressing.current = false;
      }}
    >
      <Heading level={2} className="sr-only">
        {w.t("Matters")}
      </Heading>
      {/* New matter and Find stand on a sticky head, as the scratch matter and Settings stand on the foot */}
      <div className="room-rail-head">
        <Button
          kind="tone"
          className="w-full flex-none justify-start px-s3 text-muted is-hover:text-primary"
          onClick={onNewMatter}
        >
          <IconNewMatter size={16} />
          {w.t("New matter")}
        </Button>
        {find !== undefined && (
          <FindField
            query={query}
            onQuery={(q) => {
              setQuery(q);
              setFinding(true);
            }}
            onOpen={() => setFinding(true)}
            onClose={() => {
              if (pressing.current || query.trim() !== "") return;
              setFinding(false);
            }}
            onFirst={() => {
              const first = shown === "none" ? undefined : shown[0]?.matters[0];
              first?.onOpen?.();
            }}
          />
        )}
      </div>
      {all.length === 0 && !finding ? (
        <NoMatters />
      ) : shown === "none" ? (
        <div className="room-find-none" role="status">
          <p className="text-ui font-chip text-primary">
            <Say>{mt(w.lang, "No matter matches '{q}'.", { q: query.trim() })}</Say>
          </p>
          <p className="text-small text-quiet">
            <Say>{mt(w.lang, "Search covers names, clients and case numbers.")}</Say>
          </p>
        </div>
      ) : (
        <div
          className={cn("room-groups", find !== undefined && "room-groups-find")}
          data-finding={finding ? "" : undefined}
        >
          {shown.map((g) => (
            <section key={g.when}>
              <Heading level={2} depth={1} className="room-when">
                {g.when}
              </Heading>
              <ul className="room-matters">
                {g.matters.map((m) => (
                  <MatterRow key={m.name} {...m} />
                ))}
              </ul>
            </section>
          ))}
        </div>
      )}
      {/* with Settings, the scratch matter and Settings stand on the rail's foot, always in view as the list scrolls */}
      {settings === undefined && notice === undefined ? (
        scratchRow
      ) : (
        <div className="room-rail-foot">
          {notice}
          {scratchRow}
          {/* key caps stand on inline prompts only: the shortcut is named to assistive technology and under the pointer */}
          {settings !== undefined && (
            <a
              className="room-settings"
              href={settings}
              title={platform === "mac" ? "⌘," : "Ctrl+,"}
              aria-keyshortcuts={platform === "mac" ? "Meta+Comma" : "Control+Comma"}
            >
              <span className="deva-nudge">{ui(w.lang, "Settings")}</span>
            </a>
          )}
        </div>
      )}
    </nav>
  );
}
