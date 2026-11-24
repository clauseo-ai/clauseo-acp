// The panel beside the list. With nothing picked, what this part is, for a lawyer who has never met
// the word; with an item picked, everything about it, read-only (L1): where it came from, its line,
// what stands in its way, its notes, how Clauseo reaches one of the lawyer's own connectors, and
// where the assistant uses it.
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { cr } from "@clauseo/components/lib/words-connectors-room.ts";
import { lb } from "@clauseo/components/lib/words-library.ts";
import { CASE_LAW } from "../store/connectors.ts";
import type { MatterId } from "../transport/protocol.ts";
import type { AddonView } from "./addons.ts";
import type { Assistant } from "./assistant.ts";
import { CaseLawBlocks, RecheckButton, type CaseLawActs } from "./case-law.tsx";
import { caseLawShown } from "./case-law-model.ts";
import { ItemTile } from "./glyphs.tsx";
import type { Part } from "./model.ts";
import { descriptionOf, fromLine, nameOf, scopeTag, troubleLine } from "./model.ts";

export function Explainer({ part, who }: { part: Part; who: string }) {
  const { lang } = useDisplay();
  if (part === "skills")
    return (
      <div className="lib-explain" data-testid="library-explainer">
        <h3 className="lib-explain-head">{lb(lang, "What a skill is")}</h3>
        <p>
          {lb(
            lang,
            "A skill is a page of instructions for one kind of work, such as drafting a legal notice the way your chamber does. When a question calls for it, {who} reads the skill and follows it.",
            { who },
          )}
        </p>
        <p>
          {lb(
            lang,
            "Most skills are instructions only. A skill that holds programs says so, because {who} may run those programs on this computer.",
            { who },
          )}
        </p>
        <h3 className="lib-explain-head">{lb(lang, "Where skills come from")}</h3>
        <p>
          {lb(
            lang,
            "skills.sh lists skills people have shared. A colleague can also send you a skill as a folder or a zip file.",
          )}
        </p>
      </div>
    );
  return (
    <div className="lib-explain" data-testid="library-explainer">
      <h3 className="lib-explain-head">{lb(lang, "What a connector is")}</h3>
      <p>
        {lb(
          lang,
          "A connector lets {who} reach something outside the matter's papers: a case-law database, a website, a program your firm runs. When one is used, the conversation shows it as a step.",
          { who },
        )}
      </p>
      <h3 className="lib-explain-head">{lb(lang, "Your own connectors are MCP servers")}</h3>
      <p>
        {lb(
          lang,
          "MCP is the common way apps such as Claude Desktop connect to outside tools, so one made for those apps works here too. You add it by its address on the internet, or by the command that starts it on this computer.",
        )}
      </p>
      <p>
        {lb(
          lang,
          "It runs with the same access you have: your files, your programs and the internet. Add only connectors you trust.",
        )}
      </p>
    </div>
  );
}

/** How Clauseo reaches one of the lawyer's own connectors: its address or command, and its saved
 * values by name. */
function ToolFacts({ a }: { a: AddonView }) {
  const { lang } = useDisplay();
  const f = a.from;
  if (f === undefined || (f.kind !== "address" && f.kind !== "program")) return null;
  const value = f.kind === "program" ? [f.text, ...(a.server?.args ?? [])].join(" ") : f.text;
  const secrets = [...(a.server?.env ?? []), ...(a.server?.headers ?? [])].map((s) => s.name);
  return (
    <section className="lib-detail-block">
      <h4 className="lib-detail-label">{lb(lang, "How Clauseo reaches it")}</h4>
      <dl className="lib-facts">
        <dt>{f.kind === "address" ? lb(lang, "Address") : lb(lang, "Command")}</dt>
        <dd className="lib-mono" lang="en">
          <bdi>{value}</bdi>
        </dd>
        {secrets.length > 0 && (
          <>
            <dt>{lb(lang, "Saved values")}</dt>
            <dd lang="en">
              {secrets.map((s) => (
                <span key={s} className="lib-secret lib-mono">
                  {s}
                </span>
              ))}
            </dd>
          </>
        )}
      </dl>
    </section>
  );
}

export function ItemDetail({
  a,
  names,
  assistant,
  caseLaw,
}: {
  a: AddonView;
  names: ReadonlyMap<MatterId, string>;
  assistant: Assistant;
  caseLaw: CaseLawActs;
}) {
  const { lang } = useDisplay();
  const isCaseLaw = a.id === CASE_LAW && a.source === "built_in";
  // Case Law's own state decides its line: the firm's off is "blocked", the lawyer's off is said in
  // its blocks, and before the connectors' state arrives nothing is said yet
  const shown = isCaseLaw ? caseLawShown(a, caseLaw.facts) : null;
  const status = shown === "trouble" && caseLaw.facts.off === "firm" ? "blocked" : a.status;
  const trouble =
    shown === "off" || shown === "waiting" || shown === "added"
      ? null
      : troubleLine(lang, status, assistant.who);
  const description = descriptionOf(lang, a);
  const where = scopeTag(lang, a, names);
  const showsWhere =
    where !== "" && a.status !== "invalid" && a.status !== "needs_setup" && !isCaseLaw;
  return (
    <div className="lib-detail" data-testid="library-detail">
      <header className="lib-detail-head">
        <ItemTile addon={a} big />
        <div className="min-w-0">
          <h3 className="lib-detail-name">
            <bdi>{nameOf(lang, a)}</bdi>
          </h3>
          <p className="text-small text-muted">
            {isCaseLaw ? cr(lang, "Powered by Indian Kanoon") : fromLine(lang, a)}
          </p>
        </div>
      </header>
      {description !== "" && <p className="lib-detail-desc">{description}</p>}
      {trouble !== null && (
        <div className="lib-trouble" role="status">
          <p>{trouble}</p>
          {shown === "unreachable" && <RecheckButton recheck={caseLaw.recheck} small />}
        </div>
      )}
      {a.has_scripts === true && (
        <p className="lib-note">
          {lb(
            lang,
            "This skill holds programs. {Who} may run them on this computer, with the same access you have.",
            { Who: assistant.Who },
          )}
        </p>
      )}
      {a.id === "browser" && a.source === "built_in" && (
        <p className="lib-note">
          {lb(
            lang,
            "It comes with its own skill, Using the browser, with rules for using a browser on legal work. The sites it may open are in Settings, under Allowed sites.",
          )}
        </p>
      )}
      {isCaseLaw && <CaseLawBlocks a={a} acts={caseLaw} who={assistant.who} />}
      {a.kind === "tool" && a.source === "added" && <ToolFacts a={a} />}
      {showsWhere && (
        <section className="lib-detail-block">
          <h4 className="lib-detail-label">
            {lb(lang, "Where {who} uses it", { who: assistant.who })}
          </h4>
          <p className="text-ui text-primary" data-testid="library-detail-where">
            {where}
          </p>
        </section>
      )}
    </div>
  );
}
