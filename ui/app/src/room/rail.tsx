// The room's rail: the parts' Rail with a MatterRow for every matter the host lists (matters.ts), the scratch matter
// only at its foot. With the NextHost's matters part, the date groups (when.ts), Pinned, Recent (the five opened last),
// finding by name and client, and each row's menu (Pin, Locate folder, Show in Finder, Remove from the list); without it,
// one group, "No date yet", newest first, finding by name only, and a menu of Show in Finder alone. What each row says it's doing comes from matters.ts's status,
// its moments put in the lawyer's clock as the rail renders, so nothing ticks at rest; the open matter's row says
// nothing, since the lawyer is looking at it. Until the list arrives, and if it can't be read, the rail draws no list
// and no scratch row. Under the open matter's row stand its conversations (conversations.ts), with New conversation:
// a draft, or a thread with no title yet, says "New conversation"; a row not open says what it's doing.
import type { Ref } from "react";
import { Rail, type MatterRowProps, type RailGroup } from "@clauseo/components/clauseo/rail.tsx";
import type { ConversationRowProps } from "@clauseo/components/clauseo/rail-conversations.tsx";
import { mt } from "@clauseo/components/lib/words-matters.ts";
import type { Lang } from "@clauseo/components/lib/display.ts";
import { frame } from "@clauseo/components/lib/words-frame.ts";
import { useWords } from "@clauseo/components/lib/words.ts";
import { isolate } from "@clauseo/components/lib/bidi.ts";
import type { MatterId } from "../transport/protocol.ts";
import type { MatterInfo } from "./host-next.ts";
import { planName } from "./agent.ts";
import { rowWords } from "./conversation-status.ts";
import type { ConversationRow, Conversations, ConversationsView } from "./conversations.ts";
import type { Matters, MattersView, RowStatus } from "./matters.ts";
import { clockOf, dayOf, groupMatters } from "./when.ts";

/** A row's status as MatterRow takes it: the state, and its moment as words. Working never names a time (no end
 * estimates), so it says "Answering."; a stop or a reset on another day gives its day. */
function stateWords(
  status: RowStatus | undefined,
  now: number,
  zone: string,
  lang: Lang,
): Pick<MatterRowProps, "state" | "at" | "day"> {
  if (status === undefined) return { state: "idle" };
  if (status.at === undefined) return { state: status.state, at: "", day: "" };
  const { time, day } = clockOf(status.at, now, zone, lang);
  return { state: status.state, at: time, day };
}

/** The open matter's conversations as ConversationList takes them. A draft takes the plan it would run. A row's moment
 * goes into the lawyer's clock here, as the matters' do. A stop on another day says nothing: the row has no room for a
 * day, and "Stopped at 4:34 pm" alone would read as today, while "Stopped at" with no time reads as nothing. */
function conversationRows(
  talks: ConversationsView,
  conversations: Conversations,
  onConversation: () => void,
  now: number,
  zone: string,
  lang: Lang,
): ConversationRowProps[] {
  const untitled = mt(lang, "New conversation");
  const titleOf = (row: ConversationRow | undefined) => row?.title ?? untitled;
  return talks.rows.map((row) => {
    const words = rowWords(row);
    const stopped = words.state === "stopped" ? clockOf(words.at, now, zone, lang) : null;
    const state = stopped !== null && stopped.day !== "" ? "idle" : words.state;
    const extra =
      words.state === "waiting"
        ? { behind: titleOf(talks.rows.find((r) => r.thread === words.behind)) }
        : stopped !== null && state === "stopped"
          ? { at: stopped.time }
          : {};
    return {
      id: row.key,
      title: titleOf(row),
      agent: planName(row.agent ?? talks.agent, talks.agents),
      state,
      ...extra,
      current: row.key === talks.open,
      testId: "conversation",
      onOpen: () => {
        conversations.open(row.key);
        onConversation();
      },
    };
  });
}

/** How many matters Recent shows: the five opened last. */
const RECENT = 5;

/** The newest first, by id: v7 ids sort by creation. */
const newestFirst = (a: MatterInfo, b: MatterInfo) =>
  a.matter < b.matter ? 1 : a.matter > b.matter ? -1 : 0;

export function RoomRail({
  matters,
  view,
  conversations,
  talks,
  open,
  onNewMatter,
  onMoved,
  onConversation,
  onStartNew,
  newMatterRef,
  onEscape,
  onSettings,
}: {
  matters: Matters;
  view: MattersView;
  conversations: Conversations;
  /** The open matter's conversations. */
  talks: ConversationsView;
  /** Matters, pressed below 1200 px: the rail stands open over the work. */
  open: boolean;
  onNewMatter: () => void;
  /** A matter was opened from the rail. */
  onMoved: () => void;
  /** A conversation was opened, or a new one started, from the rail: the folded rail closes, and the field takes the
   * focus the conversations ask for. */
  onConversation: () => void;
  /** Heard just before New conversation starts one from the rail: the room ends Versions, back to now. False: Versions
   * can't end yet (its dialog asks, or a restore or undo runs), and no conversation starts. */
  onStartNew?: (() => boolean) | undefined;
  newMatterRef: Ref<HTMLButtonElement>;
  /** Escape inside the rail. */
  onEscape: () => void;
  /** Settings at the rail's foot, pressed; left out where there is no settings window to open (outside the desktop
   * app), and the rail draws no Settings. */
  onSettings?: (() => void) | undefined;
}) {
  const w = useWords();
  const { now, zone } = view;
  const detailed = view.detailed;
  const row = (m: MatterInfo): MatterRowProps => {
    const current = view.open?.matter === m.matter;
    return {
      id: m.matter,
      name: m.name,
      note: "",
      hint:
        m.folder_display === undefined || m.folder_display === "" ? "" : isolate(m.folder_display),
      current,
      // the menu's Locate folder follows the folder, apart from the status: the open row says nothing, yet offers it
      moved: m.folder_state === "missing",
      // the matter the lawyer is looking at says nothing: its work is in view
      ...(current
        ? { state: "idle" as const }
        : stateWords(view.status.get(m.matter), now, zone, w.lang)),
      testIds: { row: "matter-row" },
      // the open matter's conversations, once its list is in
      ...(current && talks.matter === m.matter && talks.phase !== "loading"
        ? {
            conversations: conversationRows(
              talks,
              conversations,
              onConversation,
              now,
              zone,
              w.lang,
            ),
            onNewConversation: () => {
              // the button stays as it is rather than greying: Versions' dialog is modal and stands while its act
              // runs, so the rail can't be pressed then anyway; this only guards it, and nothing happens
              if (onStartNew?.() === false) return;
              conversations.startNew();
              onConversation();
            },
            conversationPlan: talks.plan,
            newConversationTestId: "new-conversation",
          }
        : {}),
      onOpen: () => {
        matters.open(m.matter);
        onMoved();
      },
      // Show in Finder (File Explorer): the menu greys it while the folder has moved
      onReveal: () => void matters.reveal(m.matter),
      ...(detailed
        ? {
            pinned: m.pinned,
            onPin: (pinned: boolean) => void matters.pin(m.matter, pinned),
            onRemove: () => void matters.forget(m.matter),
            ...(m.folder_state === "missing"
              ? { onLocate: () => void matters.locate(m.matter) }
              : {}),
          }
        : {}),
    };
  };
  const folders = view.matters.filter((m) => m.kind !== "scratch");
  const scratch = view.matters.find((m) => m.kind === "scratch");
  const pinned = detailed ? folders.filter((m) => m.pinned) : [];
  const groups: RailGroup[] = detailed
    ? groupMatters(
        folders.filter((m) => !m.pinned),
        dayOf(now, zone),
      ).map((g) => ({ when: frame(w.lang, g.when), matters: g.matters.map(row) }))
    : [
        {
          when: frame(w.lang, "No date yet"),
          matters: [...folders].sort(newestFirst).map(row),
        },
      ];
  const recent = detailed
    ? folders
        .filter((m) => m.last_opened !== undefined)
        .sort((a, b) => (b.last_opened ?? 0) - (a.last_opened ?? 0))
        .slice(0, RECENT)
        .map(row)
    : [];
  const all = folders.map((m) => ({
    ...row(m),
    client: detailed ? (m.client ?? "") : "",
    caseNo: "",
  }));
  const openScratch = (id: MatterId) => () => {
    matters.open(id);
    onMoved();
  };
  return (
    <Rail
      groups={groups}
      pinned={pinned.map(row)}
      open={open}
      onNewMatter={onNewMatter}
      newMatterRef={newMatterRef}
      // nothing to find on an empty rail
      {...(folders.length === 0
        ? {}
        : {
            find: { all, recent, covers: detailed ? ("nameClient" as const) : ("name" as const) },
          })}
      {...(scratch === undefined
        ? {}
        : {
            scratch: {
              current: view.open?.matter === scratch.matter,
              onOpen: openScratch(scratch.matter),
            },
          })}
      empty={view.phase === "ready"}
      onEscape={onEscape}
      {...(onSettings === undefined ? {} : { onSettings })}
      testIds={{ newMatter: "new-matter", find: "find", settings: "open-settings" }}
    />
  );
}
