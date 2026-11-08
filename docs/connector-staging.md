# Testing the case-law connector against staging

The recipe for the release-checklist row "real Claude and Codex calling case law through the
connector on macOS and Windows". It **waits for the connector's staging deploy**
(`https://data-staging.clauseo.chat`) **and a staging test account**; until both exist, the row
stays open.

## The staging build

The connector's origin is fixed when the app is compiled (`crates/desktop/src/connectors.rs`,
`ORIGIN`). The `staging-connector` feature of `clauseo-desktop`, off by default, points it at
the staging host. No setting, policy, environment variable or argument changes it, so a
release build can never be switched to staging, and a staging build never talks to production.
The feature turns on `clauseo-connectors`' `staging`, which alone compiles in `Origin::Staging`
and the staging host: a build without it holds no `data-staging.clauseo.chat` string at all.

```sh
pnpm -C ui tauri build --features staging-connector
```

Run it from the repo root on the Mac and on the Windows machine, the way `release.yml` builds
(`--no-bundle` skips the installer when the binary is enough). What changes in a staging build:

- every connector request (sign-in, MCP, price list, account, recharge) goes to the staging
  host, with the user agent `Clauseo/<version> (staging)`;
- `OpenPayLink` opens only `https://data-staging.clauseo.chat/pay/<id>`; a production pay link
  is refused, as a staging one is in a production build.

Install it on a test machine only, never next to a lawyer's install: it shares the data folder
and keychain item with the release app.

## Signing in

1. Open a matter, then Settings, Connectors, Case-law searches.
2. **Sign in** opens the staging sign-in page in the browser. Sign in with the staging test
   account (ask whoever holds the staging deploy for it; its credentials stay out of
   `measurements/`, screenshots and logs).
3. The section reads "Ready to search." with the test account's balance. Staging credits are
   test credits: nothing here costs money.

## The checklist row

Run each step for **Claude** and for **Codex**, each in a matter, on **macOS** and on
**Windows** (four runs).

1. **Handshake.** Start a session in the matter. The connector's tools (`search_law` and the
   rest) appear in the agent's tool list: the relay took the session's token. Claude calls
   them without a permission prompt (`mcp__case-law` is pre-allowed).
2. **A paid `search_law`.** Ask for a short search ("find one Supreme Court judgment on
   anticipatory bail"). The result comes back with its sources, and Settings' balance drops by
   the search's price after **Check balance**.
3. **The turn cap (60 paid calls).** In one message ask for 61 separate one-result searches.
   Call 61 answers "This turn has reached Clauseo's limit of 60 case-law calls…", and the agent
   stops calling. A new message can search again.
4. **The day cap (600 a day).** Quit the app, set `<data>/connectors/day.json` to
   `{"day":"<today, YYYY-MM-DD>","paid":599}`, start it again. One search goes through; the
   next answers "Clauseo's daily limit of 600 case-law calls on this computer is reached…".
   Delete the file afterwards.
5. **Sign-out mid-session.** With the session open, Settings, Connectors, **Sign out**. The
   agent's next case-law call answers "Case law is signed out. Tell the lawyer to sign in again
   under Settings, Connectors." (not reachable), and the session itself carries on.
6. **AddCredits, never paid.** Sign in again, **Add credits**, choose the cheapest pack,
   **Continue to payment**, then **Open the payment page**. The native confirm names
   `data-staging.clauseo.chat`; accept it and check the browser shows the staging pay page.
   Close it without paying. The section keeps "Open the payment page again" until the recharge
   expires.

## The local build (Mac mini)

Before staging is up, the same row runs against the connector's local test server on the Mac
mini, at exactly `http://127.0.0.1:8793` (on Windows, a port forward to it on the same
address). The `local-connector` feature, off by default and exclusive with
`staging-connector` (the build fails with both), points the connector there. Like staging it
is chosen only at compile time, and a build without it holds no `127.0.0.1:8793`
(`crates/desktop/tests/staging_host.rs`).

```sh
cargo run -p clauseo-desktop --features local-connector        # a dev run
pnpm -C ui tauri build --features local-connector               # or the bundle
```

What changes in a local build:

- every connector request goes to `http://127.0.0.1:8793`, on the production paths, with no
  proxy and no `Origin` header, and the user agent `Clauseo/<version> (local)`;
- plain http is allowed for exactly that origin: the OAuth issuer and `iss` must be exactly
  `http://127.0.0.1:8793`, and every endpoint (authorize, token, revoke) and the client document
  must be on it; another host or port, or https, is refused;
- the sign-in page and `OpenPayLink` open only that origin's links, and the pay link only as
  `http://127.0.0.1:8793/pay/<id>`.

Start the local server on 127.0.0.1:8793 first, then sign in as above with its test account and
run the six steps. Record the runs as below, with `local` in the folder name for the origin.

## What to record

In the private repo's `measurements/connector-staging/<date>-<os>-<agent>/` (one folder per
run; `<date>-local-<os>-<agent>` for a local build), with no token, key, account id or e-mail in any of it:

- `run.md`: the app commit, the build command, the OS and version, the agent and its version,
  and each step's pass or fail, with the exact text the agent got at steps 3 to 5;
- a screenshot of the tool list (step 1), of the result (step 2) and of the pay page's confirm
  (step 6);
- the balance before and after step 2, and the price list's search price;
- anything that differed from this recipe.
