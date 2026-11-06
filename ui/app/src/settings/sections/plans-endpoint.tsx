// Plans' firm address (UD2), Claude only, under "Other ways in": a URL field showing the address set
// (SettingsView.plans["claude-acp"].endpoint), Connect, which sends `set_firm_endpoint { url }`, and once one is set
// Remove address, which sends it with no url. Desktop shows its own confirm for both, so the app adds none; a
// declined one, or a refused address, leaves the block's refusal line and keeps what was typed.
import { useState } from "react";
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { st } from "@clauseo/components/lib/words-settings.ts";
import { say } from "@clauseo/components/lib/words-ui.ts";
import { Button } from "@clauseo/components/ui/button.tsx";
import { TextField } from "@clauseo/components/ui/text-field.tsx";

export function PlansEndpoint({
  endpoint,
  onConnect,
}: {
  endpoint: string | undefined;
  /** The address to set, or undefined to remove it. */
  onConnect: (url: string | undefined) => void;
}) {
  const { lang } = useDisplay();
  const [typed, setTyped] = useState(endpoint ?? "");
  const url = typed.trim();
  return (
    <div className="set-plan-endpoint" data-testid="plans-endpoint">
      <TextField
        label={st(lang, "Your firm's address")}
        note={st(
          lang,
          "Your IT team gives you this. Questions go through your firm's own account, on its terms.",
        )}
        value={typed}
        onValueChange={setTyped}
        placeholder="https://"
        lang="en"
      />
      <div className="set-plan-acts">
        <Button
          kind="tone"
          size="small"
          disabled={url === "" || url === endpoint}
          onClick={() => onConnect(url)}
        >
          {say(st(lang, "Connect"))}
        </Button>
        {endpoint !== undefined && (
          <Button kind="quiet" size="small" onClick={() => onConnect(undefined)}>
            {say(st(lang, "Remove address"))}
          </Button>
        )}
      </div>
    </div>
  );
}
