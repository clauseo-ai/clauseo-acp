// The details an error on the working line folds open, under its row, with the row's Details trigger. Its own
// chunk: most rows have no details.
import type { ReactNode } from "react";
import { Collapsible as BaseCollapsible } from "@base-ui/react/collapsible";
import { Button } from "../ui/button.tsx";
import { CollapsiblePanel } from "../ui/collapsible.tsx";
import { Say } from "./say.tsx";
import { IconFold } from "../icons/icons.tsx";
import { useThreadWords } from "../lib/words-thread.ts";

export function WorkingDetails({
  row,
  details,
}: {
  /** The working line's row, given the trigger to set among its acts. */
  row: (more: ReactNode) => ReactNode;
  details: string;
}) {
  const w = useThreadWords();
  return (
    <BaseCollapsible.Root className="working-wrap">
      {row(
        <BaseCollapsible.Trigger
          className="working-more"
          render={<Button kind="quiet" size="small" />}
        >
          <span className="deva-nudge">{w.th("Details")}</span>
          <IconFold size={12} className="work-chev" />
        </BaseCollapsible.Trigger>,
      )}
      <CollapsiblePanel>
        <p className="working-details">
          <Say>{details}</Say>
        </p>
      </CollapsiblePanel>
    </BaseCollapsible.Root>
  );
}
