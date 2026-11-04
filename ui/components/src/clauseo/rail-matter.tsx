// A matter's row on the rail: its name as filed, its note, why a search found it and what it's doing, with its
// actions in a context menu, and, for the current matter, its conversations opened beneath it on one raised plane.
import { ContextMenu } from "@base-ui/react/context-menu";
import { Menu as BaseMenu } from "@base-ui/react/menu";
import type { ReactNode } from "react";
import { Button } from "../ui/button.tsx";
import {
  MenuCheckboxItem,
  MenuGroup,
  MenuGroupLabel,
  MenuItem,
  MenuPopup,
  MenuSeparator,
  MenuTrigger,
  Menu,
} from "../ui/menu.tsx";
import { useLayer } from "../ui/layer-context.ts";
import { raised } from "../ui/recipes-overlays.ts";
import {
  IconLocateFolder,
  IconMore,
  IconNotProtected,
  IconPin,
  IconRemoveFromList,
  IconShowInFinder,
} from "../icons/icons.tsx";
import { cn } from "../lib/cn.ts";
import { langOfText } from "../lib/script.ts";
import { useDisplay } from "../lib/display.ts";
import { useWords } from "../lib/words.ts";
import { revealKey, ui } from "../lib/words-ui.ts";
import { tbt } from "../lib/words-toolbar.ts";
import { Say } from "./say.tsx";
import { ConversationList, type ConversationRowProps } from "./rail-conversations.tsx";
import { RailMark } from "./rail-mark.tsx";
import type { Triage } from "./rail-triage.tsx";
import { MATTER_MARK } from "./rail-marks.ts";
import { matterStatus, type MatterState } from "./rail-words.ts";

export type { MatterState };

export type MatterRowProps = {
  /** Optional: the matter's id. The rail keys its rows by it, falling back to the name. */
  id?: string | undefined;
  /** The matter's name as filed: English in both languages. */
  name: string;
  /** Its note, in the language: "Recovery suit, for the plaintiff". */
  note: string;
  hint: string;
  current?: boolean;
  state?: MatterState;
  /** The clock time a state names, in English: "4:32 pm" expected, "4:31 pm" stopped, or the limit's reset. Working
   * with none says "Answering."; the limit with none says the reset wasn't given. */
  at?: string;
  /** Paused, limit and stopped: the day of the reset or the stop, in the language, when it isn't today. */
  day?: string;
  /** Optional: its folder isn't where it was, for its actions (Locate folder), apart from what its status line says:
   * the open matter's row says nothing, yet its menu still offers Locate folder. Left out, the moved state says it. */
  moved?: boolean | undefined;
  /** Pinned above every date group. Its menu's Pin to the top shows it ticked. */
  pinned?: boolean | undefined;
  /** Optional: in Needs you, the title of the conversation behind its status, quiet, beneath the status's words. */
  where?: string | undefined;
  /** A line under the note saying why a search found it, where the name alone doesn't show: "Client, Asha Rao". */
  match?: string | undefined;
  /** Optional: protection is off for this matter, or couldn't be set up. The row's name line ends in the open
   * lock, 14 px in concrete, named "Not protected" to assistive technology and under the pointer, for as long as that
   * lasts. */
  unprotected?: boolean | undefined;
  /** The row pressed: the room moves to this matter. */
  onOpen?: () => void;
  /** Its actions, from the context menu or, on the current matter, the chevron at its end. Each shows only with its
   * handler: Pin to the top with onPin, Locate folder with onLocate while the folder has moved, Show in Finder (or
   * Explorer) with onReveal, Remove from the list with onRemove. With none of them, the row has no menu at all. */
  onPin?: ((pinned: boolean) => void) | undefined;
  onLocate?: (() => void) | undefined;
  onReveal?: (() => void) | undefined;
  onRemove?: (() => void) | undefined;
  /** Optional: test ids; row goes on the row's button, which carries data-state. */
  testIds?: { row?: string } | undefined;
  /** The current matter's conversations, opened beneath it, with New conversation. */
  conversations?: readonly ConversationRowProps[] | undefined;
  onNewConversation?: (() => void) | undefined;
  /** Optional: the plan the conversations are measured against (ConversationList's `plan`): its first conversation's,
   * where known. */
  conversationPlan?: string | undefined;
  /** Optional: a test id on New conversation. */
  newConversationTestId?: string | undefined;
  /** Optional (U16b): the current matter's done conversations, folded under its list (ConversationList's `done`). */
  conversationsDone?: { count: number; rows: () => ReactNode } | undefined;
  /** Optional (U16b): Snooze until… and Mark all done in its menu, for its conversations not done. */
  triage?: Triage | undefined;
  /** Optional (U16b): what stands in the row's place: the line after a snooze or Done, or the snooze picker. */
  replace?: ReactNode;
};

/** A matter's actions: pin it to the top, show its folder in the computer's own file manager, or find a folder that
 * moved, and take it off the list, which leaves the folder where it is. */
type ActionProps = Pick<
  MatterRowProps,
  "name" | "pinned" | "state" | "moved" | "onPin" | "onLocate" | "onReveal" | "onRemove" | "triage"
>;

/** Whether the folder has moved, for the actions: the flag where given, else the moved state. */
const movedOf = (moved: boolean | undefined, state: MatterState | undefined) =>
  moved ?? state === "moved";

/** Whether the row has any action to show: each item stands only with its handler. */
const hasActions = ({ state, moved, onPin, onLocate, onReveal, onRemove, triage }: ActionProps) =>
  onPin !== undefined ||
  triage !== undefined ||
  (movedOf(moved, state) && onLocate !== undefined) ||
  onReveal !== undefined ||
  onRemove !== undefined;

function MatterActions({
  name,
  pinned,
  state,
  moved: movedFlag,
  onPin,
  onLocate,
  onReveal,
  onRemove,
  triage,
}: ActionProps) {
  const { lang, platform } = useDisplay();
  const moved = movedOf(movedFlag, state);
  const locate = moved ? onLocate : undefined;
  const above =
    onPin !== undefined || triage !== undefined || locate !== undefined || onReveal !== undefined;
  return (
    <>
      {above && (
        <MenuGroup>
          <MenuGroupLabel lang="en">{name}</MenuGroupLabel>
          {onPin !== undefined && (
            <MenuCheckboxItem
              label={ui(lang, "Pin to the top")}
              icon={<IconPin size={16} />}
              checked={pinned === true}
              onCheckedChange={(on) => onPin(on)}
            />
          )}
          {triage?.items(true)}
          {locate !== undefined && (
            <MenuItem
              label={ui(lang, "Locate folder")}
              icon={<IconLocateFolder size={16} />}
              onClick={locate}
            />
          )}
          {onReveal !== undefined && (
            <MenuItem
              label={ui(lang, revealKey(platform))}
              icon={<IconShowInFinder size={16} />}
              disabled={moved}
              onClick={onReveal}
            />
          )}
        </MenuGroup>
      )}
      {above && onRemove !== undefined && <MenuSeparator />}
      {onRemove !== undefined && (
        <MenuItem
          label={ui(lang, "Remove from the list")}
          icon={<IconRemoveFromList size={16} />}
          note={ui(lang, "The folder and its papers stay where they are.")}
          onClick={onRemove}
        />
      )}
    </>
  );
}

/** The same actions on a press of the context key or a right-click anywhere on the row. */
function RowContextMenu(p: ActionProps) {
  const layer = useLayer();
  return (
    <ContextMenu.Portal container={layer}>
      <ContextMenu.Positioner collisionPadding={8} className="z-20">
        <BaseMenu.Popup className={raised({ width: "menu" })}>
          <MatterActions {...p} />
        </BaseMenu.Popup>
      </ContextMenu.Positioner>
    </ContextMenu.Portal>
  );
}

/** The row's own words: the name as filed, the note, why a search found it, and what it's doing. */
function RowWords({
  name,
  note,
  match,
  state,
  status,
  where,
  unprotected,
}: {
  name: string;
  note: string;
  match?: string | undefined;
  where?: string | undefined;
  state: MatterState;
  status: string | null;
  unprotected?: boolean | undefined;
}) {
  const w = useWords();
  // waiting on the lawyer, or an answer waiting to be opened: the status line speaks up
  const news =
    state === "question" ||
    state === "permission" ||
    state === "ready" ||
    state === "paused" ||
    state === "failed";
  // limit stays muted: nothing for the lawyer to choose, only a time to wait for
  return (
    <>
      <span className="room-matter-name">
        {/* a matter's name as filed: English in both languages, unless it is written in Devanagari */}
        {langOfText(name) === "hi" ? (
          <span lang="hi">{name}</span>
        ) : (
          <span className="as-en" lang="en">
            {name}
          </span>
        )}
        {unprotected === true && (
          <span className="room-matter-np" title={tbt(w.lang, "Not protected")}>
            <IconNotProtected size={14} />
            <span className="sr-only">{tbt(w.lang, "Not protected")}</span>
          </span>
        )}
      </span>
      <span className="room-matter-note">
        <Say>{note}</Say>
      </span>
      {match !== undefined && (
        <span className="room-matter-match">
          <Say>{match}</Say>
        </span>
      )}
      {status !== null && state !== "idle" && (
        <span
          className={cn(
            "flex items-start gap-s1-5 text-small",
            news ? "text-primary font-chip" : "text-muted",
          )}
        >
          <RailMark mark={MATTER_MARK[state]} className="room-matter-mark" />
          <span className="min-w-0 deva-nudge">
            <Say>{status}</Say>
            {where !== undefined && (
              <span className="room-matter-where" lang={langOfText(where)}>
                <Say>{where}</Say>
              </span>
            )}
          </span>
        </span>
      )}
    </>
  );
}

/** A matter on the rail: its name, its note beneath, and what it's doing, if anything, beneath that. The current
 * matter stands raised, its name at 520. Words carry every state, each led by its icon in the same place (rail-mark.tsx);
 * colour comes in only where it means the court's thing: a yellow disc for a matter waiting on the lawyer. */
export function MatterRow({
  name,
  note,
  hint,
  current = false,
  state = "idle",
  at = "",
  day = "",
  moved,
  pinned,
  match,
  where,
  unprotected,
  onOpen,
  onPin,
  onLocate,
  onReveal,
  onRemove,
  testIds,
  conversations,
  onNewConversation,
  conversationPlan,
  newConversationTestId,
  triage,
  replace,
  conversationsDone,
}: MatterRowProps) {
  const w = useWords();
  if (replace !== undefined && !(current && conversations !== undefined)) return <li>{replace}</li>;
  const status = matterStatus(w.lang, state, at, day);
  const actions: ActionProps = {
    name,
    pinned,
    state,
    moved,
    onPin,
    onLocate,
    onReveal,
    onRemove,
    triage,
  };
  const menu = hasActions(actions);
  const button = (
    <button
      type="button"
      className="room-matter-row"
      title={hint}
      aria-current={current ? "page" : undefined}
      data-state={state}
      data-testid={testIds?.row}
      onClick={onOpen}
    >
      <RowWords
        name={name}
        note={note}
        match={match}
        where={where}
        state={state}
        status={status}
        unprotected={unprotected}
      />
    </button>
  );
  // the current matter, opened: its row and its conversations, where given, stand on one raised plane, its actions at
  // its end, so they are reached by keyboard on every computer, not only by a context key
  if (current && (conversations !== undefined || menu)) {
    return (
      <li className="room-house">
        <div className="room-house-head">
          {replace ?? button}
          {menu && replace === undefined && (
            <Menu>
              <MenuTrigger
                render={
                  <Button
                    kind="quiet"
                    size="small"
                    shape="icon"
                    className="room-house-more data-popup-open:bg-press"
                  />
                }
                aria-label={ui(w.lang, "Actions for {name}", { name })}
              >
                <IconMore size={16} />
              </MenuTrigger>
              <MenuPopup align="start">
                <MatterActions {...actions} />
              </MenuPopup>
            </Menu>
          )}
        </div>
        {conversations !== undefined && (
          <ConversationList
            matter={name}
            conversations={conversations}
            plan={conversationPlan}
            newTestId={newConversationTestId}
            done={conversationsDone}
            {...(onNewConversation === undefined ? {} : { onNew: onNewConversation })}
          />
        )}
      </li>
    );
  }
  if (!menu) return <li>{button}</li>;
  return (
    <ContextMenu.Root>
      <ContextMenu.Trigger render={<li />}>{button}</ContextMenu.Trigger>
      <RowContextMenu {...actions} />
    </ContextMenu.Root>
  );
}
