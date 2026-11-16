// Settings, Privacy, "Allowed sites" (0053, PR 2): each site the lawyer allowed past the
// unsafe-site list, as `allowed_sites` lists it (always ones first): the host, how far the allow
// reaches (one matter by its name, or always) and when it was allowed, with a Remove that sends
// `revoke_allowed_site` and reads the list again. A site Clauseo couldn't save (`saved: false`)
// says it lasts until Clauseo closes. An empty list says so; a refusal says its code's words, but
// `blocklist.revoke_not_saved` is no refusal: the site is gone, and a quiet line names it and says
// it may be allowed again next time Clauseo opens. The list is read as the section opens, after
// each Remove and when the window regains focus (allowed-sites-commands.ts); allowing is the
// conversation's (room/thread/site-blocked.tsx).
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { bl, blocklistDay } from "@clauseo/components/lib/words-blocklist.ts";
import { isolate } from "@clauseo/components/lib/words-core.ts";
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { say } from "@clauseo/components/lib/words-ui.ts";
import { Button } from "@clauseo/components/ui/button.tsx";
import { settingLabel, settingNote } from "@clauseo/components/ui/recipes-controls.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import type { AllowedSite } from "../../transport/protocol.ts";
import { REVOKE_NOT_SAVED, useAllowedSites, useMatterNames } from "./allowed-sites-commands.ts";
import { Group, PlateRow } from "./parts.tsx";

const keyOf = (site: AllowedSite) => `${site.scope} ${site.matter ?? ""} ${site.host}`;

export function AllowedSites({ transport }: { transport: HostTransport }) {
  const { lang } = useDisplay();
  const { sites, failed, removing, unsaved, remove } = useAllowedSites(transport);
  const names = useMatterNames(transport);
  if (sites === null && failed === null) return null;
  const reach = (site: AllowedSite) => {
    if (site.scope === "always") return bl(lang, "Always");
    const name = site.matter === undefined ? undefined : names.get(site.matter);
    return name === undefined
      ? bl(lang, "One matter")
      : bl(lang, "This matter: {matter}", { matter: isolate(name) });
  };
  return (
    <div data-testid="allowed-sites" className="contents">
      <Group
        title={bl(lang, "Allowed sites")}
        note={bl(lang, "Sites you allowed although they're on the unsafe-site list.")}
      >
        {sites !== null && sites.length === 0 && (
          <PlateRow>
            <p className={settingNote} data-testid="allowed-sites-empty">
              {say(bl(lang, "No sites are allowed past the unsafe-site list."))}
            </p>
          </PlateRow>
        )}
        {sites?.map((site) => (
          <PlateRow key={keyOf(site)} className="flex items-center justify-between gap-3">
            <div className="min-w-0" data-testid="allowed-site">
              <p className={settingLabel}>
                <bdi>{site.host}</bdi>
              </p>
              <p className={settingNote}>
                {say(reach(site))}
                {" · "}
                {say(bl(lang, "Allowed {date}", { date: blocklistDay(site.allowed_at, lang) }))}
                {!site.saved && (
                  <span data-testid="allowed-site-unsaved">
                    {" · "}
                    {say(bl(lang, "Until Clauseo closes"))}
                  </span>
                )}
              </p>
            </div>
            <Button
              kind="quiet"
              size="small"
              data-testid="allowed-site-remove"
              aria-label={bl(lang, "Remove {host}", { host: isolate(site.host) })}
              disabled={removing !== null}
              onClick={() => remove(site)}
            >
              {bl(lang, "Remove")}
            </Button>
          </PlateRow>
        ))}
        {unsaved !== null && (
          <PlateRow>
            <div className="min-w-0" role="status" data-testid="allowed-sites-unsaved">
              <p className={settingLabel}>
                <bdi>{unsaved.host}</bdi>
              </p>
              <p className={settingNote}>{say(errorWords(lang, REVOKE_NOT_SAVED))}</p>
            </div>
          </PlateRow>
        )}
        {failed !== null && (
          <PlateRow>
            <p className={settingNote} role="status" data-testid="allowed-sites-failed">
              {say(errorWords(lang, failed))}
            </p>
          </PlateRow>
        )}
      </Group>
    </div>
  );
}
