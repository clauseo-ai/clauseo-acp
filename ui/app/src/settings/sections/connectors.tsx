// Settings, Connectors (U15): Indian Kanoon's block by where it stands, from the window's connectors state
// (store/connectors.ts), then its recent spend, the lawyer's own switch in a group of its own, and what each search
// sends. Its own chunk, loaded when the section opens (sections.tsx). Every button sends exactly one connector
// command through settings/connector-commands.ts, the settings window's own; Turn on and Turn off go through the
// display store's setTools as `set_tool_settings { connectors }`, and the shown switch always comes from
// ConnectorsState.available.
//
// The pay link opens after one press, with a guard (the brief's "Decided here"): "Continue to payment" sends
// `add_credits` and notes the pay_url it saw before and the press's time. When a new, different pay_url arrives while
// this same view is mounted, the packs not cancelled or changed, and within 30 s of the press, the section sends
// `open_pay_link` once; the times are compared as the state arrives, so no timer runs. Otherwise "Open the payment
// page" waits for a press, so the native confirm never comes up out of nowhere. `OutOfCredit.pay_url` is never a
// control: after out of credit, "Add credits" makes a fresh link.
import { useDisplay, type Lang } from "@clauseo/components/lib/display.ts";
import { count, rupees } from "@clauseo/components/lib/money.ts";
import { cw, type ConnectorsKey } from "@clauseo/components/lib/words-connectors.ts";
import { say } from "@clauseo/components/lib/words-ui.ts";
import { settingLabel, settingNote } from "@clauseo/components/ui/recipes-controls.ts";
import { Button } from "@clauseo/components/ui/button.tsx";
import { useEffect, useRef, useState, type ReactNode } from "react";
import { stamp } from "../../room/thread/clock.ts";
import { useNow } from "../../store/limits.ts";
import type { Balance, ConnectorView } from "../../transport/protocol.ts";
import { sendConnector, type SettingsConnectorCommand } from "../connector-commands.ts";
import type { SectionContext } from "../sections.tsx";
import { kindOf, PAY_GUARD_MS, RECHARGE_LINE } from "./connectors-model.ts";
import { shownConnector } from "./connectors-note.ts";
import { KeyField } from "./connectors-key.tsx";
import { Packs } from "./connectors-packs.tsx";
import { Sends } from "./connectors-privacy.tsx";
import { Spend } from "./connectors-spend.tsx";
import { Group, PlateRow } from "./parts.tsx";

/** The block's local view: the status, the packs open, or the key field open. */
type View = "status" | "packs" | "key";

/** "Continue to payment": the pay link seen before, and when. */
type Press = { before: string | undefined; at: number };

/** A row of actions; the first leads. */
function Actions({ children }: { children: ReactNode }) {
  return <div className="set-cn-actions">{children}</div>;
}

/** The balance as the Plan and usage tile's numeral: "10,000" then "credits left", and under it what those credits are
 * worth, GST-inclusive, when the host sends the rupees ("Worth {rupees}.", U15-15), and when the account was read
 * ("As of {time}.", U15-76, the plan card's clock form). Each line is left out when its field is absent. */
function BalanceTile({ lang, balance, low }: { lang: Lang; balance: Balance; low: boolean }) {
  const now = useNow();
  const numeral = count(balance.credits);
  const left = cw(lang, "{credits} credits left", { credits: numeral });
  return (
    <div
      className="set-window-tile set-cn-balance"
      data-state={low ? "close" : "plenty"}
      data-testid="connectors-balance"
    >
      <p className="set-window-used" aria-label={left}>
        <span className="set-numeral">{numeral}</span>
        <span className="set-window-word">{say(left.replace(numeral, "").trim())}</span>
      </p>
      {balance.paise !== undefined && (
        <p className="set-note" data-testid="connectors-worth">
          {say(cw(lang, "Worth {rupees}.", { rupees: rupees(balance.paise) }))}
        </p>
      )}
      {balance.as_of !== undefined && (
        <p className="set-note" data-testid="connectors-as-of">
          {say(cw(lang, "As of {time}.", { time: stamp(lang, balance.as_of, now) }))}
        </p>
      )}
    </div>
  );
}

export function Connectors({ ctx }: { ctx: SectionContext }) {
  const { lang } = useDisplay();
  const state = ctx.connectors;
  const c = shownConnector(state);
  if (c === undefined) return null;
  const added = c.status !== "not_added";
  return (
    <div className="set-connectors" data-testid="connectors-section">
      <Block ctx={ctx} c={c} lang={lang} />
      {state.available && c.recent_spend !== undefined && (
        <Spend spend={c.recent_spend} lang={lang} />
      )}
      {state.available && !state.locked && added && (
        <Group title={cw(lang, "On this computer")}>
          <PlateRow className="set-choice">
            <div className="set-choice-words" data-testid="connectors-switch">
              <p className={settingLabel}>{say(cw(lang, "Case-law searches"))}</p>
              <p className={settingNote}>
                {say(
                  cw(
                    lang,
                    "Turning off stops case-law searches on this computer. Your credits stay.",
                  ),
                )}
              </p>
            </div>
            <Button
              kind="quiet"
              size="small"
              className="flex-none"
              onClick={() => ctx.chooseTool("connectors", false)}
            >
              {cw(lang, "Turn off")}
            </Button>
          </PlateRow>
        </Group>
      )}
      <Sends lang={lang} limits={ctx.limits} />
    </div>
  );
}

function Block({ ctx, c, lang }: { ctx: SectionContext; c: ConnectorView; lang: Lang }) {
  const state = ctx.connectors;
  const { id } = c;
  const kind = kindOf(c.status);
  const [view, setView] = useState<View>("status");
  const [chosen, setChosen] = useState("");
  const [waiting, setWaiting] = useState(false);
  // the pay link this view opened, by itself or by a press
  const [opened, setOpened] = useState<string | null>(null);
  const press = useRef<Press | null>(null);
  const send = (command: SettingsConnectorCommand) =>
    sendConnector(ctx.transport, command).catch(() => {});
  const w = (key: ConnectorsKey, slots?: Record<string, string>) => say(cw(lang, key, slots));
  const payUrl = c.pay_url;
  // a signed-in status on the firm's pasted key says so; an account's, or none from an older host, says Ready to search
  const signedInWords = (method: string | undefined) =>
    method === "api_key" ? w("Using your firm's key.") : w("Ready to search.");
  // the last watched recharge's outcome: a quiet line while the wire says it (an unknown word shows nothing)
  const recharge = c.last_recharge === undefined ? undefined : RECHARGE_LINE[c.last_recharge];

  // a new pay link: open it once if this view's press asked for it under 30 s ago; else wait for a press
  useEffect(() => {
    const pressed = press.current;
    if (payUrl === undefined || pressed === null || payUrl === pressed.before) return;
    press.current = null;
    setWaiting(false);
    setView("status");
    if (Date.now() - pressed.at > PAY_GUARD_MS) return;
    setOpened(payUrl);
    void sendConnector(ctx.transport, { open_pay_link: { id } }).catch(() => {});
  }, [payUrl, id, ctx.transport]);

  const voidPress = () => {
    press.current = null;
    setWaiting(false);
  };
  const openPacks = () => {
    setChosen(c.packs[0]?.pack ?? "");
    voidPress();
    setView("packs");
  };
  const toContinue = () => {
    if (chosen === "") return;
    press.current = { before: payUrl, at: Date.now() };
    setWaiting(true);
    sendConnector(ctx.transport, { add_credits: { id, pack: chosen } }).catch(voidPress);
  };
  const openPay = () => {
    if (payUrl === undefined) return;
    setOpened(payUrl);
    void send({ open_pay_link: { id } });
  };

  const accountRow = (
    <>
      <Button kind="quiet" size="small" onClick={() => void send({ sign_out: { id } })}>
        {cw(lang, "Sign out")}
      </Button>
      <Button kind="quiet" size="small" onClick={() => void send({ remove: { id } })}>
        {cw(lang, "Remove")}
      </Button>
    </>
  );
  const keyLink = (
    <Button kind="link" size="small" onClick={() => setView("key")}>
      {cw(lang, "Use an API key instead")}
    </Button>
  );
  const keyField = (
    <KeyField
      lang={lang}
      send={(key) => sendConnector(ctx.transport, { add_api_key: { id, key } })}
      onDone={() => setView("status")}
      onCancel={() => setView("status")}
    />
  );

  let body: ReactNode;
  if (!state.available) {
    // off: by the firm, or by the lawyer on this computer; an added connector can still be signed out and removed
    body = (
      <>
        <p
          className={state.locked ? settingNote : "text-ui text-primary"}
          data-testid="connectors-off"
        >
          {w(state.locked ? "Turned off by your firm." : "Turned off on this computer.")}
        </p>
        {(!state.locked || kind !== "not_added") && (
          <Actions>
            {!state.locked && (
              <Button kind="tone" size="small" onClick={() => ctx.chooseTool("connectors", true)}>
                {cw(lang, "Turn on")}
              </Button>
            )}
            {kind !== "not_added" && accountRow}
          </Actions>
        )}
      </>
    );
  } else if (kind === "not_added") {
    body = (
      <>
        {c.price_paise !== undefined && (
          <p className="set-note">
            {w("Credits are bought in packs, from {price}, including GST.", {
              price: rupees(c.price_paise),
            })}
          </p>
        )}
        {view === "key" ? (
          keyField
        ) : (
          <>
            <div className="set-cn-add">
              <Button kind="tone" size="small" onClick={() => void send({ add: { id } })}>
                {cw(lang, "Add")}
              </Button>
              <p className="set-note">{w("Your browser opens to sign in.")}</p>
            </div>
            <Actions>{keyLink}</Actions>
          </>
        )}
      </>
    );
  } else if (kind === "signing_in") {
    body = (
      <>
        <p className="text-ui text-primary">
          {w("Finish signing in on the page your browser opened.")}
        </p>
        <Actions>
          <Button kind="tone" size="small" onClick={() => void send({ add: { id } })}>
            {cw(lang, "Open the sign-in page again")}
          </Button>
        </Actions>
      </>
    );
  } else if (kind === "signed_in" || kind === "out_of_credit") {
    const signed =
      typeof c.status === "object" && "signed_in" in c.status ? c.status.signed_in : undefined;
    const low = signed?.low === true;
    body = (
      <>
        {signed === undefined ? (
          <p className="text-ui text-primary">
            {w("No credits left. Case-law searches stop until you add more.")}
          </p>
        ) : (
          <>
            <p className="text-ui text-primary">{signedInWords(signed.method)}</p>
            {signed.balance === undefined ? (
              <div className="set-cn-unread">
                <p className="set-note">{w("Balance not read yet.")}</p>
                <Button
                  kind="quiet"
                  size="small"
                  onClick={() => void send({ refresh_balance: { id } })}
                >
                  {cw(lang, "Check balance")}
                </Button>
              </div>
            ) : (
              <BalanceTile lang={lang} balance={signed.balance} low={low} />
            )}
            {low && (
              <p className="text-ui text-primary font-chip" data-testid="connectors-low">
                {w("Running low. Add credits to keep searching.")}
              </p>
            )}
          </>
        )}
        {recharge !== undefined && (
          <p className="set-note" data-testid="connectors-recharge">
            {w(recharge)}
          </p>
        )}
        {payUrl !== undefined &&
          (opened === payUrl ? (
            <div className="set-cn-pay" data-testid="connectors-paying">
              <p className="text-ui text-primary">
                {w("The payment page is open in your browser.")}
              </p>
              <p className="set-note">
                {w("Your balance updates here once the payment goes through.")}
              </p>
              <Actions>
                <Button kind="tone" size="small" onClick={openPay}>
                  {cw(lang, "Open the payment page again")}
                </Button>
              </Actions>
            </div>
          ) : (
            <Actions>
              <Button kind="tone" size="small" onClick={openPay}>
                {cw(lang, "Open the payment page")}
              </Button>
            </Actions>
          ))}
        {view === "packs" ? (
          <Packs
            lang={lang}
            packs={c.packs}
            chosen={chosen}
            waiting={waiting}
            onChoose={(pack) => {
              voidPress();
              setChosen(pack);
            }}
            onContinue={toContinue}
            onCancel={() => {
              voidPress();
              setView("status");
            }}
          />
        ) : (
          <Actions>
            {c.packs.length > 0 && (
              <Button kind="tone" size="small" onClick={openPacks}>
                {cw(lang, "Add credits")}
              </Button>
            )}
            {accountRow}
          </Actions>
        )}
      </>
    );
  } else if (kind === "signed_out") {
    body = (
      <>
        <p className="text-ui text-primary">{w("Signed out. Sign in to search case law again.")}</p>
        {view === "key" ? (
          keyField
        ) : (
          <Actions>
            <Button kind="tone" size="small" onClick={() => void send({ add: { id } })}>
              {cw(lang, "Sign in")}
            </Button>
            {keyLink}
          </Actions>
        )}
        <Actions>
          <Button kind="quiet" size="small" onClick={() => void send({ remove: { id } })}>
            {cw(lang, "Remove")}
          </Button>
        </Actions>
      </>
    );
  } else if (kind === "key_revoked") {
    body = (
      <>
        <p className="text-ui text-primary">
          {w("This API key no longer works. Add a new key, or sign in instead.")}
        </p>
        {view === "key" ? (
          keyField
        ) : (
          <Actions>
            <Button kind="tone" size="small" onClick={() => setView("key")}>
              {cw(lang, "Add a new key")}
            </Button>
            <Button kind="quiet" size="small" onClick={() => void send({ add: { id } })}>
              {cw(lang, "Sign in")}
            </Button>
          </Actions>
        )}
        <Actions>
          <Button kind="quiet" size="small" onClick={() => void send({ remove: { id } })}>
            {cw(lang, "Remove")}
          </Button>
        </Actions>
      </>
    );
  } else {
    const error = typeof c.status === "object" && "error" in c.status ? c.status.error : undefined;
    const message = error?.message ?? "";
    // a sign-in that worked but could not be saved: the block says so, and the next start needs a new sign-in
    body = (
      <>
        <p className="text-ui text-primary">
          {error?.kind === "persist_failed"
            ? w(
                "You're signed in, but Clauseo couldn't save the sign-in on this computer. Sign in again after Clauseo restarts.",
              )
            : w("The sign-in didn't go through. Try again, or use an API key instead.")}
        </p>
        {view === "key" ? (
          keyField
        ) : (
          <Actions>
            <Button kind="tone" size="small" onClick={() => void send({ add: { id } })}>
              {cw(lang, "Try again")}
            </Button>
            {keyLink}
          </Actions>
        )}
        {message !== "" && (
          <details className="set-cn-details">
            <summary className="set-note">{w("Details")}</summary>
            <p className="set-note" lang="en">
              {message}
            </p>
          </details>
        )}
      </>
    );
  }

  return (
    <Group
      title={cw(lang, "Indian Kanoon")}
      note={cw(
        lang,
        "Judgments of the Supreme Court, the High Courts and tribunals, with statutes.",
      )}
    >
      <PlateRow className="set-cn-block">
        <div className="set-cn-body" data-testid="connectors-block" data-status={kind}>
          {body}
        </div>
      </PlateRow>
    </Group>
  );
}
