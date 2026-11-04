// The rail: the matters on the bare ground, grouped by their next date, with the current matter's conversations
// opened under its row, finding a matter, and the scratch matter at its foot. Matters, their states and their next
// dates come in as props.
import { useLayoutEffect, useRef, useState, type ReactNode, type Ref } from "react";
import { Button } from "../ui/button.tsx";
import { Kbd } from "../ui/kbd.tsx";
import { searchBoxOpen, searchInput } from "../ui/recipes.ts";
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

/** A menu's popup: it stands outside the rail on the page, and focus going into it hasn't left the rail. */
const MENU = '*[role="menu"]';

/** A group of rows under its heading; testId goes on its section. */
export type RailGroup = { when: string; matters: readonly MatterRowProps[]; testId?: string };

/** A matter the rail can find: its row, with its client and case number. */
export type FindableRow = MatterRowProps & { client: string; caseNo: string };

/** The list on first launch: what goes here. New matter, above, is the way forward. What Clauseo does with the
 * folder waits until it does it (the first read), so the line says only what to add. */
function NoMatters() {
  const w = useWords();
  return (
    <div className="grid gap-s1 px-s3 mt-s5">
      <p className="text-ui font-chip text-primary deva-nudge">{frame(w.lang, "No matters yet")}</p>
      <p className="text-small text-quiet">
        <Say>{frame(w.lang, "Add a matter's folder.")}</Say>
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
   * each was opened. Empty, as where the host doesn't say when a matter was last opened, the field focused and empty
   * shows no Recent at all: the date groups stay, as they stand with the field left. */
  recent: readonly MatterRowProps[];
  /** Where the field starts: words in it, or focused and empty. */
  query?: string;
  open?: boolean;
  /** What the search covers, so its label and its no-match line promise only that: names alone, names and clients,
   * or, left out, names, clients and case numbers. */
  covers?: "name" | "nameClient";
  /** Optional: Find searching inside (U16a). Given it, the field reads "Find", shows its key (⌘K, Ctrl+K), and while
   * words are in it the list is what this draws: the room's results, which take the matching matters found here. */
  results?: (search: FindSearch) => ReactNode;
  /** Optional: the field hovered or focused, before anything is typed: the room loads its results then. */
  onPreload?: () => void;
};

/** What Find's results are drawn from: the words in the field, the matters matched here by name, client or case
 * number (their rows ready to open, each leaving Find), and the leave itself, for a result the room opens. */
export type FindSearch = {
  query: string;
  matching: readonly MatterRowProps[];
  leave: () => void;
};

/** Find's label and the line under no match, for what it covers. */
function findWords(lang: "en" | "hi", covers: RailFind["covers"]) {
  switch (covers) {
    case "name":
      return {
        label: mt(lang, "Find a matter by its name"),
        none: mt(lang, "Search covers names."),
      };
    case "nameClient":
      return {
        label: mt(lang, "Find a matter by its name or client"),
        none: mt(lang, "Search covers names and clients."),
      };
    default:
      return {
        label: mt(lang, "Find a matter by its name, client or case number"),
        none: mt(lang, "Search covers names, clients and case numbers."),
      };
  }
}

function FindField({
  label,
  placeholder,
  keys,
  onPreload,
  testId,
  query,
  onQuery,
  onOpen,
  onClose,
  onDone,
  onFirst,
}: {
  label: string;
  placeholder: string;
  /** Find's key, shown in the empty field and named to assistive technology; none where the room has no key. */
  keys?: { cap: string; aria: string } | undefined;
  onPreload?: (() => void) | undefined;
  testId: string | undefined;
  query: string;
  onQuery: (q: string) => void;
  onOpen: () => void;
  onClose: () => void;
  /** Escape on an empty field: the date groups come back, focus stays. */
  onDone: () => void;
  onFirst: () => void;
}) {
  const room = useRoom();
  return (
    <label className={cn(searchBoxOpen, "w-full min-w-0 room-find")} onPointerEnter={onPreload}>
      <IconSearch size={16} />
      <span className="sr-only">{label}</span>
      <input
        id={room.id("find")}
        type="search"
        autoComplete="off"
        className={searchInput}
        placeholder={placeholder}
        aria-keyshortcuts={keys?.aria}
        data-testid={testId}
        value={query}
        onChange={(e) => onQuery(e.target.value)}
        onFocus={() => {
          onPreload?.();
          onOpen();
        }}
        onBlur={onClose}
        onKeyDown={(e) => {
          // Escape clears the words and keeps focus here; on an empty field the date groups come back and the key goes
          // on to the rail, where a folded rail closes
          if (e.key === "Escape" && query !== "") {
            e.preventDefault();
            e.stopPropagation();
            onQuery("");
          } else if (e.key === "Escape") {
            onDone();
          } else if (e.key === "Enter") {
            e.preventDefault();
            onFirst();
          }
        }}
      />
      {keys !== undefined && query === "" && <Kbd className="room-find-key">{keys.cap}</Kbd>}
    </label>
  );
}

/** The rail: matters on the bare ground, grouped by their next date, under a full-width New matter, with Scratch
 * matter, "Quick questions", below a hairline. Pinned matters stand above every date, under "Pinned". With no
 * matters at all, the rail says so. Given find, a field under New matter finds any matter; given settings, the
 * rail's foot links to them, and given onSettings, its Settings is a button that opens them (their own window). */
export function Rail({
  groups,
  pinned = [],
  open,
  onNewMatter,
  onNewMatterPreload,
  find,
  scratch,
  settings,
  onSettings,
  notice,
  label,
  testIds,
  newMatterRef,
  empty = true,
  onEscape,
  aside,
  list,
  onHover,
  shelf,
  onFocusLeave,
}: {
  groups: readonly RailGroup[];
  pinned?: readonly MatterRowProps[];
  open: boolean;
  /** New matter, pressed. */
  onNewMatter?: () => void;
  /** Optional: New matter hovered or focused, before it is pressed (the room loads its dialog then). */
  onNewMatterPreload?: () => void;
  find?: RailFind;
  /** The scratch matter: whether it's the room, and its row pressed. Left out, the foot has no scratch row. */
  scratch?: { current?: boolean; onOpen?: () => void };
  /** Whether no matter means there are none, so the rail says "No matters yet". Default true; false while the list
   * is on its way or couldn't be read, when the rail draws no list at all. */
  empty?: boolean;
  /** Optional: Escape pressed inside the rail, with nothing in it taking the key: a folded rail closes. */
  onEscape?: () => void;
  /** Where Settings lives, linked from the rail's foot. */
  settings?: string;
  /** Optional: Settings at the rail's foot, pressed: opens them in a window of their own. Given it, Settings is a
   * button, in the link's place and look. */
  onSettings?: () => void;
  /** Optional: what sets this rail apart where several stand on one page, added to its name, "Matters, …". */
  label?: string;
  /** Optional: what the room says about Clauseo itself (an update, the first crash, a lab's terms),
   * as a RailNotice, in its own row at the rail's foot above Scratch matter. */
  notice?: ReactNode;
  /** Optional: test ids for New matter, Find's field, the scratch matter's row (which also carries
   * data-kind="scratch") and Settings. */
  testIds?: { newMatter?: string; scratch?: string; find?: string; settings?: string };
  /** Optional: New matter's button, so a dialog it opens can give focus back to it on close. */
  newMatterRef?: Ref<HTMLButtonElement>;
  /** Optional: what stands under Find on the head (Matters / Recent), hidden while Find has the focus or words. */
  aside?: ReactNode;
  /** Optional: a list in the groups' place (Recent), while Find has neither the focus nor words. */
  list?: ReactNode;
  /** Optional: the pointer came over the rail (true) or left it (false). */
  onHover?: (over: boolean) => void;
  /** Optional (U16b): the Snoozed shelf, at the foot of the groups, above Scratch matter. */
  shelf?: ReactNode;
  /** Optional (U16b): the focus left the rail for another control on the page (not for one of its menus). */
  onFocusLeave?: () => void;
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
        ? find.recent.length === 0
          ? all
          : [{ when: mt(w.lang, "Recent"), matters: find.recent.map(opening) }]
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
  const words = findWords(w.lang, find?.covers);
  // searching inside: the room's results stand in the matching matters' place while words are in the field
  const inside = find?.results !== undefined && finding && query.trim() !== "";
  const findKeys =
    find?.results === undefined
      ? undefined
      : platform === "mac"
        ? { cap: "⌘K", aria: "Meta+K" }
        : { cap: "Ctrl+K", aria: "Control+K" };
  const scratchRow = scratch !== undefined && (
    <div className="room-scratch">
      <button
        type="button"
        className="room-matter-row"
        aria-current={scratch.current === true ? "page" : undefined}
        data-kind="scratch"
        data-testid={testIds?.scratch}
        onClick={scratch.onOpen}
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
      onPointerEnter={onHover === undefined ? undefined : () => onHover(true)}
      onPointerLeave={onHover === undefined ? undefined : () => onHover(false)}
      onPointerDownCapture={() => {
        pressing.current = true;
      }}
      onPointerUp={() => {
        pressing.current = false;
      }}
      onBlur={(e) => {
        const to = e.relatedTarget;
        if (
          onFocusLeave !== undefined &&
          to instanceof Element &&
          nav.current?.contains(to) === false &&
          to.closest(MENU) === null
        )
          onFocusLeave();
      }}
      onKeyDown={(e) => {
        // a menu's popup sits outside the rail on the page, though inside it in React: its Escape is its own
        if (e.key !== "Escape" || e.defaultPrevented || onEscape === undefined) return;
        if (!(e.target instanceof Node) || nav.current?.contains(e.target) !== true) return;
        onEscape();
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
          onPointerEnter={onNewMatterPreload}
          onFocus={onNewMatterPreload}
          {...(newMatterRef === undefined ? {} : { ref: newMatterRef })}
          {...(testIds?.newMatter === undefined ? {} : { "data-testid": testIds.newMatter })}
        >
          <IconNewMatter size={16} />
          {w.t("New matter")}
        </Button>
        {find !== undefined && (
          <FindField
            label={find.results === undefined ? words.label : mt(w.lang, "Find")}
            placeholder={mt(w.lang, find.results === undefined ? "Find a matter" : "Find")}
            keys={findKeys}
            onPreload={find.onPreload}
            testId={testIds?.find}
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
            onDone={() => setFinding(false)}
            onFirst={() => {
              if (inside) {
                nav.current?.querySelector<HTMLElement>(".room-find-results button")?.click();
                return;
              }
              const first = shown === "none" ? undefined : shown[0]?.matters[0];
              first?.onOpen?.();
            }}
          />
        )}
        {!finding && aside}
      </div>
      {inside && find?.results !== undefined ? (
        <div className="room-groups room-groups-find room-find-results" data-finding="">
          {find.results({
            query,
            matching: shown === "none" ? [] : (shown[0]?.matters ?? []),
            leave,
          })}
        </div>
      ) : list !== undefined && !finding ? (
        list
      ) : shown !== "none" && shown.length === 0 && (finding || shelf === undefined) ? (
        empty && <NoMatters />
      ) : shown === "none" ? (
        <div className="room-find-none" role="status">
          <p className="text-ui font-chip text-primary">
            <Say>{mt(w.lang, "No matter matches '{q}'.", { q: query.trim() })}</Say>
          </p>
          <p className="text-small text-quiet">
            <Say>{words.none}</Say>
          </p>
        </div>
      ) : (
        <div
          className={cn("room-groups", find !== undefined && "room-groups-find")}
          data-finding={finding ? "" : undefined}
        >
          {shown.map((g) => (
            <section key={g.when} data-testid={g.testId}>
              <Heading level={2} depth={1} className="room-when">
                {g.when}
              </Heading>
              <ul className="room-matters">
                {g.matters.map((m) => (
                  <MatterRow key={m.id ?? m.name} {...m} />
                ))}
              </ul>
            </section>
          ))}
          {!finding && shelf}
        </div>
      )}
      {/* with Settings, the scratch matter and Settings stand on the rail's foot, always in view as the list scrolls */}
      {settings === undefined && onSettings === undefined && notice === undefined ? (
        scratchRow
      ) : (
        <div className="room-rail-foot">
          {notice}
          {scratchRow}
          {/* key caps stand on inline prompts only: the shortcut is named to assistive technology and under the pointer */}
          {onSettings !== undefined ? (
            <button
              type="button"
              className="room-settings"
              onClick={onSettings}
              title={platform === "mac" ? "⌘," : "Ctrl+,"}
              aria-keyshortcuts={platform === "mac" ? "Meta+Comma" : "Control+Comma"}
              data-testid={testIds?.settings}
            >
              <span className="deva-nudge">{ui(w.lang, "Settings")}</span>
            </button>
          ) : (
            settings !== undefined && (
              <a
                className="room-settings"
                href={settings}
                title={platform === "mac" ? "⌘," : "Ctrl+,"}
                aria-keyshortcuts={platform === "mac" ? "Meta+Comma" : "Control+Comma"}
                data-testid={testIds?.settings}
              >
                <span className="deva-nudge">{ui(w.lang, "Settings")}</span>
              </a>
            )
          )}
        </div>
      )}
    </nav>
  );
}
