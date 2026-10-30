// The controls that stand in place: buttons, chips, fields, choices, settings, measures and folds.
import { IconUndo, IconSearch } from "@clauseo/components/icons/icons.tsx";
import { useGalleryWords } from "./words.ts";
import { Button } from "@clauseo/components/ui/button.tsx";
import { ButtonPair, PairButton } from "@clauseo/components/ui/button-pair.tsx";
import { Checkbox } from "@clauseo/components/ui/checkbox.tsx";
import { Chip, type ChipRole } from "@clauseo/components/ui/chip.tsx";
import {
  Collapsible,
  CollapsiblePanel,
  CollapsibleTrigger,
} from "@clauseo/components/ui/collapsible.tsx";
import { SearchField } from "@clauseo/components/ui/field.tsx";
import { PlanMeter } from "@clauseo/components/ui/meter.tsx";
import { Kbd } from "@clauseo/components/ui/kbd.tsx";
import { Progress } from "@clauseo/components/ui/progress.tsx";
import { RadioGroup, RadioItem } from "@clauseo/components/ui/radio-group.tsx";
import { SegmentedSwitch } from "@clauseo/components/ui/segmented.tsx";
import { Separator } from "@clauseo/components/ui/separator.tsx";
import { RadioSetting, SettingRow } from "@clauseo/components/ui/settings-row.tsx";
import { Switch } from "@clauseo/components/ui/switch.tsx";
import { Tabs, TabsList, TabsPanel } from "@clauseo/components/ui/tabs.tsx";
import { TextField } from "@clauseo/components/ui/text-field.tsx";
import {
  Toolbar,
  ToolbarButton,
  ToolbarPair,
  ToolbarPairButton,
} from "@clauseo/components/ui/toolbar.tsx";
import { useState } from "react";
import { Section, State } from "./layout.tsx";
import { VIEWS, type LiteChoice, type View } from "./options.ts";

const ROLES: readonly ChipRole[] = [
  "relied",
  "supports",
  "against",
  "prepared",
  "accepted",
  "for",
  "cite",
  "good",
  "checking",
];

export function Pressables() {
  const [view, setView] = useState<View>("list");
  return (
    <>
      <Section title="Buttons">
        <State name="Primary">
          <Button kind="primary">Save</Button>
        </State>
        <State name="Tone">
          <Button kind="tone">
            Save <Kbd>⌘S</Kbd>
          </Button>
        </State>
        <State name="Quiet">
          <Button kind="quiet">Cancel</Button>
        </State>
        <State name="Link">
          <Button kind="link">Open</Button>
        </State>
        <State name="Small, large">
          <span className="flex items-center gap-s2">
            <Button kind="tone" size="small">
              Save
            </Button>
            <Button kind="primary" size="large">
              Save
            </Button>
          </span>
        </State>
        <State name="Icon">
          <Button kind="quiet" shape="icon" aria-label="Undo">
            <IconUndo size={16} />
          </Button>
        </State>
        <State name="Disabled">
          <span className="flex items-center gap-s2">
            <Button kind="primary" disabled>
              Save
            </Button>
            <Button kind="tone" disabled>
              Cancel
            </Button>
          </span>
        </State>
        <State name="Pair">
          <ButtonPair label="Page">
            <PairButton>Previous</PairButton>
            <PairButton>Next</PairButton>
          </ButtonPair>
        </State>
      </Section>
      <Section title="Chips, segmented, toolbar">
        <State name="Chips on the surface">
          <span className="flex flex-wrap gap-s2">
            {ROLES.map((r) => (
              <Chip key={r} role={r} />
            ))}
          </span>
        </State>
        <State name="On a tile, with a tick">
          <span className="flex flex-wrap gap-s2 bg-tile rounded-tile p-s2">
            <Chip role="supports" ground="tile" />
            <Chip role="good" ground="tile" icon="tick" />
          </span>
        </State>
        <State name="Segmented">
          <SegmentedSwitch label="View" value={view} options={VIEWS} onValueChange={setView} />
        </State>
        <State name="Segmented, disabled">
          <SegmentedSwitch
            label="View"
            value="list"
            options={VIEWS}
            onValueChange={() => {}}
            disabled
          />
        </State>
        <State name="Toolbar">
          <Toolbar label="Document">
            <ToolbarButton shape="icon" aria-label="Undo">
              <IconUndo size={16} />
            </ToolbarButton>
            <ToolbarPair label="Page">
              <ToolbarPairButton>Previous</ToolbarPairButton>
              <ToolbarPairButton>Next</ToolbarPairButton>
            </ToolbarPair>
            <SearchField label="Search" place="toolbar" />
          </Toolbar>
        </State>
      </Section>
    </>
  );
}

export function Fields() {
  const w = useGalleryWords();
  return (
    <Section title="Fields">
      <State name="Search">
        <SearchField label="Search" />
      </State>
      <State name="Search, disabled">
        <SearchField label="Search" disabled />
      </State>
      <div className="grid grid-cols-2 items-start gap-s5 w-full">
        <TextField
          label={w.ui("Folder")}
          note={w.ui("Clauseo reads only the papers in this folder.")}
          defaultValue="Documents/Lease"
          lang="en"
        />
        <TextField
          label={w.ui("Folder")}
          note={w.ui("Clauseo reads only the papers in this folder.")}
          defaultValue="Documents/Lease"
          lang="en"
          invalid
          error={w.ui("This folder has moved.")}
          action={<Button kind="link">{w.ui("Locate folder")}</Button>}
        />
        <TextField
          label={w.ui("Note")}
          note={w.ui("Shown under the matter's name in the list.")}
          multiline
        />
        <TextField
          label={w.ui("Note")}
          note={w.ui("Shown under the matter's name in the list.")}
          disabled
        />
      </div>
    </Section>
  );
}

export function Choices() {
  const [on, setOn] = useState(true);
  const [off, setOff] = useState(false);
  const [pick, setPick] = useState("a");
  return (
    <Section title="Checkbox, radio, switch">
      <State name="Checkbox: on, off, disabled">
        <span className="flex items-center gap-s3">
          <Checkbox label="On" checked={on} onCheckedChange={setOn} />
          <Checkbox label="Off" checked={off} onCheckedChange={setOff} />
          <Checkbox label="Disabled on" checked disabled onCheckedChange={() => {}} />
          <Checkbox label="Disabled off" checked={false} disabled onCheckedChange={() => {}} />
        </span>
      </State>
      <State name="Radio: chosen, not, disabled">
        <RadioGroup
          value={pick}
          onValueChange={(v) => setPick(String(v))}
          className="flex items-center gap-s3"
        >
          <RadioItem value="a" aria-label="First" />
          <RadioItem value="b" aria-label="Second" />
          <RadioItem value="c" aria-label="Third" disabled />
        </RadioGroup>
      </State>
      <State name="Switch: on, off, disabled">
        <span className="flex items-center gap-s3">
          <Switch label="On" checked={on} onCheckedChange={setOn} />
          <Switch label="Off" checked={off} onCheckedChange={setOff} />
          <Switch label="Disabled on" checked disabled onCheckedChange={() => {}} />
          <Switch label="Disabled off" checked={false} disabled onCheckedChange={() => {}} />
        </span>
      </State>
      <State name="Separator: across, upright">
        <span className="flex items-center gap-s3 h-control w-full">
          <Separator className="w-meter" />
          <Separator orientation="vertical" />
        </span>
      </State>
      <State name="Keys">
        <span className="flex gap-s1">
          <Kbd>⌘</Kbd>
          <Kbd>K</Kbd>
        </span>
      </State>
    </Section>
  );
}

export function Settings() {
  const w = useGalleryWords();
  const [warn, setWarn] = useState(true);
  const [lite, setLite] = useState<LiteChoice>("auto");
  return (
    <Section title="Settings rows">
      <div className="grid w-full">
        <SettingRow
          control="switch"
          label={w.ui("Warn before adding a network folder")}
          note={w.ui("Papers on a network drive take longer to read and to keep.")}
          checked={warn}
          onCheckedChange={setWarn}
        />
        <SettingRow
          control="checkbox"
          label={w.ui("Warn before adding a network folder")}
          note={w.ui("Papers on a network drive take longer to read and to keep.")}
          checked={warn}
          onCheckedChange={setWarn}
        />
        <SettingRow
          control="switch"
          label={w.ui("Lite mode")}
          note={w.ui("Takes away every shadow and all motion.")}
          checked={false}
          onCheckedChange={() => {}}
          disabled
        />
        <RadioSetting
          label={w.ui("Lite mode")}
          note={w.ui("Takes away every shadow and all motion.")}
          value={lite}
          onValueChange={setLite}
          choices={[
            {
              value: "auto",
              label: w.ui("Automatic"),
              note: w.ui("On by itself over remote desktops such as Citrix"),
            },
            { value: "on", label: w.ui("On"), note: w.ui("Always, on this computer") },
            {
              value: "off",
              label: w.ui("Off"),
              note: w.ui("Never, even over a remote desktop"),
              disabled: true,
            },
          ]}
        />
      </div>
    </Section>
  );
}

export function Measures() {
  const w = useGalleryWords();
  const at = w.clock("4:30 pm");
  return (
    <Section title="Progress and meter">
      <div className="grid gap-s5 w-full">
        <Progress
          value={3}
          max={8}
          label={w.ui("Reading papers, {n} of {total}", { n: 3, total: 8 })}
        />
        <PlanMeter
          name={w.ui("Plan")}
          value={30}
          valueText="30%"
          status={w.f("Plenty left until the {time} reset", { time: at })}
        />
        <PlanMeter
          name={w.ui("Plan")}
          value={85}
          valueText="85%"
          state="close"
          status={w.f("Close to the limit. Resets at {time}.", { time: at })}
        />
        <PlanMeter
          name={w.ui("Plan")}
          value={100}
          valueText="100%"
          state="reached"
          status={w.f("Limit reached until {time}", { time: at })}
        />
      </div>
    </Section>
  );
}

export function Folds() {
  const w = useGalleryWords();
  return (
    <Section title="Tabs and folds">
      <Tabs defaultValue="settings" className="grid gap-s3 w-full">
        <TabsList
          label={w.ui("Settings")}
          tabs={[
            { value: "settings", label: w.ui("Settings") },
            { value: "plan", label: w.ui("Plan") },
            { value: "matters", label: w.ui("Matters"), disabled: true },
          ]}
        />
        <TabsPanel value="settings" className="text-small text-muted">
          {w.say(w.ui("Clauseo reads only the papers in this folder."))}
        </TabsPanel>
        <TabsPanel value="plan" className="text-small text-muted">
          {w.say(w.ui("Questions go to this plan, which you already pay for."))}
        </TabsPanel>
      </Tabs>
      <State name="Fold, open">
        <Collapsible defaultOpen>
          <CollapsibleTrigger>{w.ui("Folder")}</CollapsibleTrigger>
          <CollapsiblePanel className="text-small text-muted">
            <span className="flex items-center gap-s2 pt-s2">
              <IconSearch size={16} /> Notice to the bank.docx
            </span>
          </CollapsiblePanel>
        </Collapsible>
      </State>
      <State name="More, closed">
        <Collapsible>
          <CollapsibleTrigger kind="more">{w.t("{n} more", { n: 3 })}</CollapsibleTrigger>
          <CollapsiblePanel className="text-small text-muted">Lease.docx</CollapsiblePanel>
        </Collapsible>
      </State>
    </Section>
  );
}
