// Settings, Language: the language of every word on screen, English or हिन्दी. It changes at once: the display moves
// to it (display.ts's showDisplay), and nothing reloads.
import { useId } from "react";
import { SegmentedSwitch } from "@clauseo/components/ui/segmented.tsx";
import { LANGUAGE_NAMES, useSettingsWords } from "@clauseo/components/lib/words-settings.ts";
import type { UiLanguage } from "../../room/host-next.ts";
import type { SectionContext } from "../sections.tsx";
import { ChoiceRow, Group } from "./parts.tsx";

export function Language({ ctx }: { ctx: SectionContext }) {
  const w = useSettingsWords();
  const locked = ctx.locked("language");
  const managed = useId();
  return (
    <Group>
      <ChoiceRow
        label={w.s("Language")}
        note={w.s("Changes at once, and everything stays where it was.")}
        locked={locked}
        managedId={managed}
      >
        <SegmentedSwitch<UiLanguage>
          label={w.s("Language")}
          value={ctx.settings.ui.language}
          disabled={locked}
          describedBy={locked ? managed : undefined}
          onValueChange={(v) => ctx.choose("language", v)}
          options={[
            { value: "en", label: LANGUAGE_NAMES.en, lang: "en" },
            { value: "hi", label: LANGUAGE_NAMES.hi, lang: "hi" },
          ]}
        />
      </ChoiceRow>
    </Group>
  );
}
