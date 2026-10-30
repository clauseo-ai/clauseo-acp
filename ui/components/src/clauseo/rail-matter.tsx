// A matter's row on the rail: its name as filed, its note, why a search found it and what it's doing, with its
// actions in a context menu, and, for the current matter, its conversations opened beneath it on one raised plane.
import { ContextMenu } from "@base-ui/react/context-menu";
import { Menu as BaseMenu } from "@base-ui/react/menu";
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
import { IconMore, IconNotProtected } from "../icons/icons.tsx";
import { cn } from "../lib/cn.ts";
import { useDisplay } from "../lib/display.ts";
import { useWords } from "../lib/words.ts";
import { revealKey, ui } from "../lib/words-ui.ts";
import { vt } from "../lib/words-versions.ts";
import { Say } from "./say.tsx";
import { ConversationList, type ConversationRowProps } from "./rail-conversations.tsx";
import { MATTER_MARK, matterStatus, type MatterState } from "./rail-words.ts";

export type { MatterState };

export type MatterRowProps = {
  /** The matter's name as filed: English in both languages. */
  name: string;
  /** Its note, in the language: "Recovery suit, for the plaintiff". */
  note: string;
  hint: string;
  current?: boolean;
  state?: MatterState;
  /** The clock time a state names, in English: "4:32 pm" expected, or "4:31 pm" stopped. */
  at?: string;
  /** Paused only: the reset's day, in the language, when it isn't today, as for the weekly limit. */
  day?: string;
  /** Pinned above every date group. Its menu's Pin to the top shows it ticked. */
  pinned?: boolean | undefined;
  /** A line under the note saying why a search found it, where the name alone doesn't show: "Client, Asha Rao". */
  match?: string | undefined;
  /** Optional: protection is off for this matter, or couldn't be set up. The row's name line ends in the open
   * lock, 14 px in concrete, named "Not protected" to assistive technology and under the pointer, for as long as that
   * lasts. */
  unprotected?: boolean | undefined;
  /** The row pressed: the room moves to this matter. */
  onOpen?: () => void;
  /** Its actions, from the context menu or, on the current matter, the chevron at its end. */
  onPin?: ((pinned: boolean) => void) | undefined;
  onLocate?: (() => void) | undefined;
  /** The current matter's conversations, opened beneath it, with New conversation. */
  conversations?: readonly ConversationRowProps[] | undefined;
  onNewConversation?: (() => void) | undefined;
};

/** A matter's actions: pin it to the top, show its folder in the computer's own file manager, or find a folder that
 * moved, and take it off the list, which leaves the folder where it is. */
function MatterActions({
  name,
  pinned,
  state,
  onPin,
  onLocate,
}: Pick<MatterRowProps, "name" | "pinned" | "state" | "onPin" | "onLocate">) {
  const { lang, platform } = useDisplay();
  const moved = state === "moved";
  return (
    <>
      <MenuGroup>
        <MenuGroupLabel lang="en">{name}</MenuGroupLabel>
        <MenuCheckboxItem
          label={ui(lang, "Pin to the top")}
          checked={pinned === true}
          onCheckedChange={(on) => onPin?.(on)}
        />
        {moved && (
          <MenuItem
            label={ui(lang, "Locate folder")}
            {...(onLocate === undefined ? {} : { onClick: onLocate })}
          />
        )}
        <MenuItem label={ui(lang, revealKey(platform))} disabled={moved} />
      </MenuGroup>
      <MenuSeparator />
      <MenuItem
        label={ui(lang, "Remove from the list")}
        note={ui(lang, "The folder and its papers stay where they are.")}
      />
    </>
  );
}

/** The same actions on a press of the context key or a right-click anywhere on the row. */
function RowContextMenu(
  p: Pick<MatterRowProps, "name" | "pinned" | "state" | "onPin" | "onLocate">,
) {
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
  unprotected,
}: {
  name: string;
  note: string;
  match?: string | undefined;
  state: MatterState;
  status: string | null;
  unprotected?: boolean | undefined;
}) {
  const w = useWords();
  // waiting on the lawyer, or an answer waiting to be opened: the status line speaks up
  const news =
    state === "question" || state === "permission" || state === "ready" || state === "paused";
  return (
    <>
      <span className="room-matter-name">
        <span className="as-en" lang="en">
          {name}
        </span>
        {unprotected === true && (
          <span className="room-matter-np" title={vt(w.lang, "Not protected")}>
            <IconNotProtected size={14} />
            <span className="sr-only">{vt(w.lang, "Not protected")}</span>
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
          <i
            aria-hidden="true"
            className={cn("flex-none size-dot-hint mt-s1-5 rounded-full", MATTER_MARK[state])}
          />
          <span className="min-w-0 deva-nudge">
            <Say>{status}</Say>
          </span>
        </span>
      )}
    </>
  );
}

/** A matter on the rail: its name, its note beneath, and what it's doing, if anything, beneath that. The current
 * matter stands raised, its name at 520. Words carry every state, each led by the same dot in the same place; colour
 * comes in only where it means the court's thing: yellow for a matter waiting on the lawyer. */
export function MatterRow({
  name,
  note,
  hint,
  current = false,
  state = "idle",
  at = "",
  day = "",
  pinned,
  match,
  unprotected,
  onOpen,
  onPin,
  onLocate,
  conversations,
  onNewConversation,
}: MatterRowProps) {
  const w = useWords();
  const status = matterStatus(w.lang, state, at, day);
  const actions = { name, pinned, state, onPin, onLocate };
  const button = (
    <button
      type="button"
      className="room-matter-row"
      title={hint}
      aria-current={current ? "page" : undefined}
      data-state={state}
      onClick={onOpen}
    >
      <RowWords
        name={name}
        note={note}
        match={match}
        state={state}
        status={status}
        unprotected={unprotected}
      />
    </button>
  );
  // the current matter, opened: its row and its conversations stand on one raised plane, its actions at its end
  if (current && conversations !== undefined) {
    return (
      <li className="room-house">
        <div className="room-house-head">
          {button}
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
        </div>
        <ConversationList
          matter={name}
          conversations={conversations}
          {...(onNewConversation === undefined ? {} : { onNew: onNewConversation })}
        />
      </li>
    );
  }
  if (onPin === undefined && onLocate === undefined) return <li>{button}</li>;
  return (
    <ContextMenu.Root>
      <ContextMenu.Trigger render={<li />}>{button}</ContextMenu.Trigger>
      <RowContextMenu {...actions} />
    </ContextMenu.Root>
  );
}
