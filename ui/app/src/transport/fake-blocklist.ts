// The unsafe-site list's host, scripted (0053, PR 2), for FakeTransport's `blocklist`: the allowed
// sites `allowed_sites` lists, and the two commands, which change them as the host would: an allow
// adds the notice's host (from the `site_blocked` events it was told of, `notice`), a revoke takes
// one away. A refusal can be scripted for each. The notices themselves are the test's to play
// (`FakeTransport.siteBlocked`).
import type { SiteBlocked } from "../room/host-next.ts";
import type { AllowScope, AllowedSite, ErrorCode, HostCommand } from "./protocol.ts";

export type FakeBlocklistHost = {
  /** Answers each `allowed_sites` query; throwing refuses it. */
  sites: () => readonly AllowedSite[];
  /** Answers `allow_blocked_site` and `revoke_allowed_site`; throwing refuses; done when left
   * out. */
  command?: (command: FakeBlocklistCommand) => void | Promise<void>;
  /** Hears each `site_blocked` the fake plays, so an allow can name its notice. */
  notice?: (site: SiteBlocked) => void;
};

export type FakeBlocklistCommand = Extract<
  HostCommand,
  { allow_blocked_site: unknown } | { revoke_allowed_site: unknown }
>;

/** Whether a command is one of the unsafe-site list's. */
export const isBlocklistCommand = (command: HostCommand): command is FakeBlocklistCommand =>
  command.allow_blocked_site !== undefined || command.revoke_allowed_site !== undefined;

const refusal = (code: ErrorCode) => ({
  code,
  message: "the fake's scripted refusal",
  retryable: false,
});

/** The refusals a fake can script, by the call's wire name. */
export type FakeBlocklistRefusals = Partial<
  Record<"allow_blocked_site" | "revoke_allowed_site" | "allowed_sites", ErrorCode>
>;

/** A stateful unsafe-site host: `refuse` names a command's scripted refusal, by its wire name. */
export function fakeBlocklist({
  sites = [],
  refuse = {},
  now = () => Date.now(),
}: {
  sites?: AllowedSite[];
  refuse?: FakeBlocklistRefusals;
  now?: () => number;
} = {}): FakeBlocklistHost & { list: () => AllowedSite[] } {
  let list = [...sites];
  const notices = new Map<string, SiteBlocked>();
  const add = (site: SiteBlocked, scope: AllowScope) => {
    const same = (s: AllowedSite) =>
      s.host === site.host && s.scope === scope && (scope === "always" || s.matter === site.matter);
    if (list.some(same)) return;
    const row: AllowedSite = {
      host: site.host,
      scope,
      ...(scope === "matter" ? { matter: site.matter } : {}),
      allowed_at: now(),
    };
    // always ones first, as the host lists them
    list = scope === "always" ? [row, ...list] : [...list, row];
  };
  return {
    notice: (site) => void notices.set(site.block_id, site),
    list: () => list,
    sites: () => {
      if (refuse.allowed_sites !== undefined) throw refusal(refuse.allowed_sites);
      return list;
    },
    command: (command) => {
      const allow = command.allow_blocked_site;
      if (allow !== undefined) {
        if (refuse.allow_blocked_site !== undefined) throw refusal(refuse.allow_blocked_site);
        const site = notices.get(allow.block_id);
        if (site === undefined) throw refusal("blocklist.notice_expired");
        add(site, allow.scope);
        return;
      }
      const revoke = command.revoke_allowed_site;
      if (revoke === undefined) return;
      if (refuse.revoke_allowed_site !== undefined) throw refusal(refuse.revoke_allowed_site);
      list = list.filter(
        (s) => !(s.host === revoke.host && s.scope === revoke.scope && s.matter === revoke.matter),
      );
    },
  };
}
