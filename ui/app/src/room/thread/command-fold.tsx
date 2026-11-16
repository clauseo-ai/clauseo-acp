// A task's command, folded (D-017, D-016): a quiet press, "Show the command", that opens the command under the words
// in the code face, its paths already cut to the matter (task-words.ts). The step row and the permission prompt
// both lead with plain words and keep this one press for the detail.
import { useId, useState } from "react";
import { Say } from "@clauseo/components/clauseo/say.tsx";
import type { Lang } from "@clauseo/components/lib/display.ts";
import { tr } from "@clauseo/components/lib/words-tool-result.ts";

export function CommandFold({ command, lang }: { command: string; lang: Lang }) {
  const [open, setOpen] = useState(false);
  const block = useId();
  return (
    <>
      <button
        type="button"
        className="step-row-press"
        aria-expanded={open}
        aria-controls={block}
        data-testid="task-command-press"
        onClick={() => setOpen((o) => !o)}
      >
        <Say>{tr(lang, open ? "Hide the command" : "Show the command")}</Say>
      </button>
      {open && (
        <span id={block} className="step-row-result" data-face="code" data-testid="task-command">
          <bdi>
            <span className="step-row-result-text" dir="ltr">
              {command}
            </span>
          </bdi>
        </span>
      )}
    </>
  );
}
