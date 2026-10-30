// The matters' parts: the rail's rows and conversations, the queue and the steering line, the new matter form and its
// dialog, the usage limit's notes, and the plan's card. The rail, queue and limit samples sit in their own files.
import { useState } from "react";
import { useWords } from "@clauseo/components/lib/words.ts";
import { Button } from "@clauseo/components/ui/button.tsx";
import { NewMatterForm } from "@clauseo/components/clauseo/matter-form.tsx";
import { NewMatterDialog } from "@clauseo/components/clauseo/new-matter-dialog.tsx";
import { PlanCard } from "@clauseo/components/clauseo/plan-card.tsx";
import { Group, Sample } from "./frame.tsx";
import { LimitSamples } from "./matters-limits.tsx";
import { folderFor, WINDOWS } from "./matters-data.ts";
import { QueueSamples } from "./matters-queue.tsx";
import { RailSamples } from "./matters-rail.tsx";

/** The rail's New matter, closed on load: pressed, it opens the dialog. */
function NewMatterTrigger() {
  const w = useWords();
  const [open, setOpen] = useState(false);
  return (
    <>
      <Button kind="tone" size="small" onClick={() => setOpen(true)}>
        {w.t("New matter")}
      </Button>
      <NewMatterDialog open={open} onOpenChange={setOpen} onCreate={() => {}} />
    </>
  );
}

const PLAN_LINK = { label: "Manage plan", href: "https://example.com/plan" };

export function MattersParts() {
  return (
    <Group title="Matters">
      <RailSamples />
      <QueueSamples />
      <Sample name="New matter form, first launch">
        <NewMatterForm folderFor={folderFor} onCreate={() => {}} />
      </Sample>
      <Sample name="New matter form, filled in">
        <NewMatterForm
          folderFor={folderFor}
          name="Acme Ltd v. State"
          client="Acme Ltd"
          next="30 October 2026"
          titled={false}
          onCreate={() => {}}
          onCancel={() => {}}
        />
      </Sample>
      <Sample name="New matter, the dialog closed">
        <NewMatterTrigger />
      </Sample>
      <LimitSamples />
      <Sample name="Plan card, two windows">
        <PlanCard
          plan="Claude plan"
          updated="As of 4:28 pm"
          windows={WINDOWS}
          foot="At the limit, Claude stops where it is."
          manage={PLAN_LINK}
        />
      </Sample>
      <Sample name="Plan card, figures old">
        <PlanCard
          plan="Claude plan"
          updated="As of 2:10 pm"
          stale
          windows={WINDOWS}
          message={{
            title: "Couldn't refresh just now.",
            body: "Clauseo will try again.",
            action: (
              <Button kind="quiet" size="small">
                Try now
              </Button>
            ),
          }}
          foot="At the limit, Claude stops where it is."
          manage={PLAN_LINK}
        />
      </Sample>
      <Sample name="Plan card, preparing ahead paused">
        <PlanCard
          plan="Claude plan"
          updated="As of 4:28 pm"
          windows={WINDOWS}
          preparing={{
            title: "Preparing ahead is paused",
            text: "Clauseo won't start the next answer until you ask.",
            link: (
              <Button kind="link" size="small">
                Turn it on
              </Button>
            ),
          }}
          foot="At the limit, Claude stops where it is."
          manage={PLAN_LINK}
        />
      </Sample>
      <Sample name="Plan card, billed by use">
        <PlanCard
          plan="Claude API account"
          updated={null}
          windows={[]}
          foot={null}
          manage={{ label: "Manage account", href: "https://example.com/account" }}
        />
      </Sample>
    </Group>
  );
}
