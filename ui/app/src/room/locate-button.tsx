// Locate folder beside a refusal's words (act-refusals.ts): the moved folder's own words and icon
// (moved.tsx), as a quiet small button, the way an error line's Show in folder stands.
import { IconLocateFolder } from "@clauseo/components/icons/icons.tsx";
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { ui } from "@clauseo/components/lib/words-ui.ts";
import { Button } from "@clauseo/components/ui/button.tsx";

export function LocateButton({
  onLocate,
  className,
}: {
  onLocate: () => void;
  className?: string;
}) {
  const { lang } = useDisplay();
  return (
    <Button
      kind="quiet"
      size="small"
      {...(className === undefined ? {} : { className })}
      onClick={onLocate}
      data-testid="locate-folder"
    >
      <IconLocateFolder size={16} />
      <span className="deva-nudge">{ui(lang, "Locate folder")}</span>
    </Button>
  );
}
