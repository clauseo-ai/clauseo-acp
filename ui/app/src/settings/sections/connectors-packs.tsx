// Add credits' packs (U15): "Choose a pack", one radio per pack in the host's order, credits first and the gross
// price after ("200 credits, ₹23.60"; PackView.paise is GST-inclusive), "Prices include GST.", then "Continue to payment" and "Cancel". It opens in
// place under the balance. The section owns what Continue does (the pay link's guard, connectors.tsx); this part only
// says which pack and when.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { cw } from "@clauseo/components/lib/words-connectors.ts";
import { Button } from "@clauseo/components/ui/button.tsx";
import { RadioSetting } from "@clauseo/components/ui/settings-row.tsx";
import type { PackView } from "../../transport/protocol.ts";
import { packLabel } from "./connectors-model.ts";

export function Packs({
  lang,
  packs,
  chosen,
  waiting,
  onChoose,
  onContinue,
  onCancel,
}: {
  lang: Lang;
  packs: readonly PackView[];
  /** The pack chosen, by id. */
  chosen: string;
  /** Continue was pressed and the pay link hasn't come yet. */
  waiting: boolean;
  onChoose: (pack: string) => void;
  onContinue: () => void;
  onCancel: () => void;
}) {
  return (
    <div className="set-cn-packs" data-testid="connectors-packs">
      <RadioSetting
        className="py-0"
        label={cw(lang, "Choose a pack")}
        note={cw(lang, "Prices include GST.")}
        value={chosen}
        choices={packs.map((p) => ({ value: p.pack, label: packLabel(lang, p) }))}
        onValueChange={onChoose}
      />
      <div className="set-cn-actions">
        <Button kind="tone" size="small" disabled={waiting || chosen === ""} onClick={onContinue}>
          {cw(lang, "Continue to payment")}
        </Button>
        <Button kind="quiet" size="small" onClick={onCancel}>
          {cw(lang, "Cancel")}
        </Button>
      </div>
    </div>
  );
}
