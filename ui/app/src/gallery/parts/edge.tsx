// The edge of the matter, in the room's group: the prompt Clauseo asks in before it goes beyond the matter, the
// refusals that stop it, Not protected, and what the matter allows. The computer's own dialog only opens on a press.
import { EdgePrompt, RefusalPause } from "@clauseo/components/clauseo/edge.tsx";
import { NotProtectedMark } from "@clauseo/components/clauseo/allowed.tsx";
import { EdgeFlow, NoteSample, RefusalFlow, WorkspaceSample } from "./edge-samples.tsx";
import { FOLDER, NOW, OTHER_SITE, REFUSALS, SITE } from "./edge-data.ts";
import { Group, Sample } from "./frame.tsx";

const noop = () => {};
/** The answers a static prompt does not take: it shows its state and nothing more. */
const still = { onAllow: noop, onRefuse: noop };

export function EdgeParts() {
  return (
    <Group title="The edge">
      <Sample name="EdgePrompt, folder, asking">
        <EdgeFlow kind="folder" place={FOLDER.place} keys />
      </Sample>
      <Sample name="EdgePrompt, website, asking">
        <EdgeFlow kind="site" place={SITE} />
      </Sample>
      <Sample name="EdgePrompt, protection off, asking">
        <EdgeFlow kind="off" place="" />
      </Sample>
      <Sample name="EdgePrompt, folder, confirming">
        <EdgePrompt kind="folder" place={FOLDER.place} state={{ kind: "confirming" }} {...still} />
      </Sample>
      <Sample name="EdgePrompt, website, allowed for this matter">
        <EdgePrompt
          kind="site"
          place={SITE}
          state={{ kind: "allowed", remembered: true, at: NOW }}
          {...still}
        />
      </Sample>
      <Sample name="EdgePrompt, folder, allowed this time">
        <EdgePrompt
          kind="folder"
          place={FOLDER.place}
          state={{ kind: "allowed", remembered: false, at: NOW }}
          {...still}
        />
      </Sample>
      <Sample name="EdgePrompt, folder, not allowed this time">
        <EdgePrompt
          kind="folder"
          place={FOLDER.place}
          state={{ kind: "refused", remembered: false }}
          {...still}
        />
      </Sample>
      <Sample name="EdgePrompt, website, not allowed for this matter">
        <EdgePrompt
          kind="site"
          place={OTHER_SITE}
          state={{ kind: "refused", remembered: true }}
          {...still}
        />
      </Sample>
      <Sample name="EdgePrompt, protection off, switched off">
        <EdgePrompt
          kind="off"
          place=""
          state={{ kind: "allowed", remembered: false, at: NOW }}
          {...still}
        />
      </Sample>
      <Sample name="EdgePrompt, protection off, kept">
        <EdgePrompt kind="off" place="" state={{ kind: "refused", remembered: false }} {...still} />
      </Sample>
      <Sample name="RefusalPause, 3 in a row, asking">
        <RefusalFlow />
      </Sample>
      <Sample name="RefusalPause, 20 in all, asking">
        <RefusalPause
          count="total"
          refused={REFUSALS}
          earlier={17}
          state={{ kind: "pending" }}
          onGoOn={noop}
          onStop={noop}
        />
      </Sample>
      <Sample name="RefusalPause, stopped">
        <RefusalPause
          count="row"
          refused={REFUSALS}
          earlier={0}
          state={{ kind: "stopped", at: NOW }}
          onGoOn={noop}
          onStop={noop}
        />
      </Sample>
      <Sample name="RefusalPause, went on inside the matter">
        <RefusalPause
          count="row"
          refused={REFUSALS}
          earlier={0}
          state={{ kind: "goneOn" }}
          onGoOn={noop}
          onStop={noop}
        />
      </Sample>
      <Sample name="NotProtectedNote, protection off">
        <NoteSample why="off" />
      </Sample>
      <Sample name="NotProtectedNote, couldn't be set up">
        <NoteSample why="setup" />
      </Sample>
      <Sample name="NotProtectedMark, full">
        <NotProtectedMark />
      </Sample>
      <Sample name="NotProtectedMark, short">
        <NotProtectedMark short />
      </Sample>
      <Sample name="WorkspaceLine, safe workspace">
        <WorkspaceSample unprotected={null} />
      </Sample>
      <Sample name="WorkspaceLine, not protected, switched off">
        <WorkspaceSample unprotected={{ why: "off", since: NOW }} />
      </Sample>
      <Sample name="WorkspaceLine, not protected, couldn't be set up">
        <WorkspaceSample unprotected={{ why: "setup" }} />
      </Sample>
    </Group>
  );
}
