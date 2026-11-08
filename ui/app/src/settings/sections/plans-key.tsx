// Plans' API key (UD2), under "Other ways in": UD1's key panel where no key is stored (its field, its words, where the
// key is kept), and with one in use "Change key" (the same panel; a key sent from this window replaces the stored
// one) and "Remove key", which asks first. The key goes from the field into one `set_agent_key` and is dropped: the
// field keeps it only until desktop takes it (PasteRow), and no store, log or state holds it.
import { useRef, useState } from "react";
import { Note, PasteRow, Ways } from "@clauseo/components/clauseo/first-launch/parts.tsx";
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { fl } from "@clauseo/components/lib/words-first-launch.ts";
import { st } from "@clauseo/components/lib/words-settings.ts";
import { say, ui } from "@clauseo/components/lib/words-ui.ts";
import { Button } from "@clauseo/components/ui/button.tsx";
import { Confirm } from "./plans-parts.tsx";

export function PlansKey({
  keys,
  keysKnown,
  inUse,
  onKey,
  onRemove,
}: {
  /** Where this computer keeps a key, or null where it has no keyring. */
  keys: "mac" | "windows" | "linux" | null;
  /** Whether the host has said (setup_info): before it, nothing shows. */
  keysKnown: boolean;
  /** The plan is signed in with a stored key. */
  inUse: boolean;
  onKey: (key: string) => Promise<boolean>;
  onRemove: () => void;
}) {
  const { lang } = useDisplay();
  const [changing, setChanging] = useState(false);
  const changeButton = useRef<HTMLButtonElement>(null);
  if (!keysKnown) return null;
  if (keys === null)
    return (
      <p className="set-plan-fine">
        {say(fl(lang, "This computer has no keyring to keep an API key in."))}
      </p>
    );
  const panel = (
    <div className="set-plan-key fl-gate">
      <div className="fl-gate-body">
        <PasteRow
          label={fl(lang, "API key")}
          secret
          go={fl(lang, "Use this key")}
          testId="plans-key"
          onGo={(key) =>
            onKey(key).then((ok) => {
              if (ok) setChanging(false);
              return ok;
            })
          }
          cancel={
            inUse ? (
              <Button
                kind="quiet"
                className="fl-gate-cancel"
                onClick={() => {
                  setChanging(false);
                  changeButton.current?.focus();
                }}
              >
                {ui(lang, "Cancel")}
              </Button>
            ) : undefined
          }
        />
        <Ways>
          <Note>
            {`${fl(lang, "A key from Claude or ChatGPT, or one from your firm's IT team. Billed to the key's own account.")} ${fl(
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
  if (!inUse) return panel;
  return (
    <>
      {changing ? (
        panel
      ) : (
        <div className="set-plan-acts">
          <Button ref={changeButton} kind="quiet" size="small" onClick={() => setChanging(true)}>
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
      )}
    </>
  );
}
