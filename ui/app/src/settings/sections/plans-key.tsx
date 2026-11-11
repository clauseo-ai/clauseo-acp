// Plans' API key (UD2): one quiet "Use an API key instead" under the plan, as Connectors has it (connectors.tsx), which
// opens UD1's key panel (its field named by the key's maker, its words, where the key is kept) with a Cancel; with a
// key in use "Change key" (the same panel; a key sent from this window replaces the stored one) and "Remove key", which
// asks first. The key goes from the field into one `set_agent_key` and is dropped: the field keeps it only until
// desktop takes it (PasteRow), and no store, log or state holds it. The block holds whether the panel is open, so the
// gate's own key link (a stopped sign-in's) opens this panel too.
import { useEffect, useRef } from "react";
import type { Plan } from "@clauseo/components/clauseo/first-launch/gate.tsx";
import { KEY_LABEL, NAME } from "@clauseo/components/clauseo/first-launch/plan-names.ts";
import { Note, PasteRow, Ways } from "@clauseo/components/clauseo/first-launch/parts.tsx";
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { fl } from "@clauseo/components/lib/words-first-launch.ts";
import { st } from "@clauseo/components/lib/words-settings.ts";
import { say, ui } from "@clauseo/components/lib/words-ui.ts";
import { Button } from "@clauseo/components/ui/button.tsx";
import { Confirm } from "./plans-parts.tsx";

export function PlansKey({
  plan,
  keys,
  keysKnown,
  inUse,
  open,
  onOpen,
  onKey,
  onRemove,
}: {
  /** Whose key: the label and the help name this plan's maker only. */
  plan: Plan;
  /** Where this computer keeps a key, or null where it has no keyring. */
  keys: "mac" | "windows" | "linux" | null;
  /** Whether the host has said (setup_info): before it, nothing shows. */
  keysKnown: boolean;
  /** The plan is signed in with a stored key. */
  inUse: boolean;
  /** The key panel is open. */
  open: boolean;
  onOpen: (open: boolean) => void;
  onKey: (key: string) => Promise<boolean>;
  onRemove: () => void;
}) {
  const { lang } = useDisplay();
  const box = useRef<HTMLDivElement>(null);
  const was = useRef(open);
  // the panel's field takes the focus as it opens; as it closes, the button that opened it takes it back, unless the
  // focus went elsewhere meanwhile
  useEffect(() => {
    const before = was.current;
    was.current = open;
    const el = box.current;
    if (el === null || before === open) return;
    if (open) {
      el.querySelector<HTMLElement>("input, button")?.focus();
      return;
    }
    const at = el.ownerDocument.activeElement;
    if (at !== null && at !== el.ownerDocument.body && !el.contains(at)) return;
    el.querySelector<HTMLElement>("button")?.focus();
  }, [open]);
  if (!keysKnown) return null;
  const cancel = (
    <Button kind="quiet" className="fl-gate-cancel" onClick={() => onOpen(false)}>
      {ui(lang, "Cancel")}
    </Button>
  );
  const panel =
    keys === null ? (
      <div className="set-plan-acts">
        <p className="set-plan-fine">
          {say(fl(lang, "This computer has no keyring to keep an API key in."))}
        </p>
        {cancel}
      </div>
    ) : (
      <div className="set-plan-key fl-gate">
        <div className="fl-gate-body">
          <PasteRow
            label={fl(lang, KEY_LABEL[plan])}
            secret
            go={fl(lang, "Use this key")}
            testId="plans-key"
            onGo={(key) =>
              onKey(key).then((ok) => {
                if (ok) onOpen(false);
                return ok;
              })
            }
            cancel={cancel}
          />
          <Ways>
            <Note>
              {`${st(lang, "A key from {name}, yours or your firm's. Billed to that key's account.", { name: NAME[plan] })} ${fl(
                lang,
                keys === "mac"
                  ? "Kept in this Mac's Keychain."
                  : keys === "windows"
                    ? "Kept in Windows Credential Manager."
                    : "Kept in this computer's keyring.",
              )}`}
            </Note>
          </Ways>
        </div>
      </div>
    );
  return (
    <div ref={box} className="set-plan-ways">
      {open ? (
        panel
      ) : inUse ? (
        <div className="set-plan-acts">
          <Button kind="quiet" size="small" onClick={() => onOpen(true)}>
            {say(st(lang, "Change key"))}
          </Button>
          <Confirm
            label={st(lang, "Remove key")}
            title={st(lang, "Remove the API key?")}
            body={st(lang, "Questions wait until you sign in or add a key again.")}
            onConfirm={onRemove}
            testId="plans-key-remove"
          />
        </div>
      ) : (
        <div>
          <Button
            kind="link"
            size="small"
            onClick={() => onOpen(true)}
            data-testid="plans-key-open"
          >
            {say(fl(lang, "Use an API key instead"))}
          </Button>
        </div>
      )}
    </div>
  );
}
