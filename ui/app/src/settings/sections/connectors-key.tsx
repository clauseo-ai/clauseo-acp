// The API key (U15): a password field, from "Use an API key instead" or "Add a new key". Its words live only in the
// field's own DOM state: "Add key" reads them from the form as it's sent, puts them in the one `add_api_key` command
// and keeps them nowhere else, not in React state, a store, a log or the review page's state. The field clears only
// when the host took the key, so a refused one can be corrected; "Add key" is off while the command is out.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { cw } from "@clauseo/components/lib/words-connectors.ts";
import { Button } from "@clauseo/components/ui/button.tsx";
import { TextField } from "@clauseo/components/ui/text-field.tsx";
import { useState, type FormEvent } from "react";

/** The form's field name: what the submit reads. */
const KEY = "key";

export function KeyField({
  lang,
  send,
  onDone,
  onCancel,
}: {
  lang: Lang;
  /** Sends `add_api_key` with the key; rejects when the host refused it. */
  send: (key: string) => Promise<void>;
  /** The host took the key: the field has cleared. */
  onDone: () => void;
  onCancel: () => void;
}) {
  const [sending, setSending] = useState(false);
  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (sending) return;
    const form = event.currentTarget;
    const key = new FormData(form).get(KEY);
    if (typeof key !== "string" || key.trim() === "") return;
    setSending(true);
    send(key.trim()).then(
      () => {
        form.reset();
        setSending(false);
        onDone();
      },
      () => setSending(false),
    );
  };
  return (
    <form className="set-cn-key" onSubmit={submit} data-testid="connectors-key">
      <TextField
        secret
        name={KEY}
        lang="en"
        label={cw(lang, "API key")}
        note={cw(lang, "Your firm's key for Clauseo's case-law service. Kept on this computer.")}
      />
      <div className="set-cn-actions">
        <Button kind="tone" size="small" type="submit" disabled={sending}>
          {cw(lang, "Add key")}
        </Button>
        <Button kind="quiet" size="small" onClick={onCancel}>
          {cw(lang, "Cancel")}
        </Button>
      </div>
    </form>
  );
}
