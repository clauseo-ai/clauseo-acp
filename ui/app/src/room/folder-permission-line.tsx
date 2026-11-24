// The guarded folder's line itself (folder-permission.ts has its state): the kind's reason with Allow, or, turned down,
// what won't work and where in System Settings to change it. A status with the synced notice's look.
import { Say } from "@clauseo/components/clauseo/say.tsx";
import { IconAbout } from "@clauseo/components/icons/icons.tsx";
import { bl } from "@clauseo/components/lib/words-blocklist.ts";
import { mf } from "@clauseo/components/lib/words-matter-form.ts";
import { useWords } from "@clauseo/components/lib/words.ts";
import { Button } from "@clauseo/components/ui/button.tsx";
import { useRef } from "react";
import { REASON, type Asked } from "./folder-permission.ts";

export function FolderPermissionLine({
  asked,
  onAllow,
}: {
  asked: Asked;
  /** Resolves true once granted: focus then goes on to Create, as the line goes. */
  onAllow: () => Promise<boolean>;
}) {
  const { lang } = useWords();
  const line = useRef<HTMLDivElement>(null);
  const press = async () => {
    const form = line.current?.closest(".matter-form");
    const granted = await onAllow();
    // the button goes either way: focus on to Create when granted, else to the line that says what changed
    if (granted) form?.querySelector<HTMLElement>('[data-testid="create-matter"]')?.focus();
    else line.current?.focus();
  };
  const denied = typeof asked.state === "object" ? asked.state.denied : null;
  return (
    <div
      ref={line}
      className="scan-place"
      role="status"
      tabIndex={-1}
      data-testid="folder-permission"
      data-permission={asked.id}
    >
      <span className="scan-place-icon" aria-hidden="true">
        <IconAbout size={16} />
      </span>
      <p className="scan-place-text">
        <Say>
          {denied === null
            ? mf(lang, REASON[asked.id])
            : mf(
                lang,
                "Without it, Clauseo can't open or save papers in this folder. You can allow it later in {path}.",
                { path: denied },
              )}
        </Say>
      </p>
      {denied === null && (
        <Button
          kind="quiet"
          size="small"
          data-testid="folder-permission-allow"
          aria-disabled={asked.state === "asking" ? true : undefined}
          aria-busy={asked.state === "asking" ? true : undefined}
          onClick={() => {
            if (asked.state === "ask") void press();
          }}
        >
          <span className="deva-nudge">{bl(lang, "Allow")}</span>
        </Button>
      )}
    </div>
  );
}
