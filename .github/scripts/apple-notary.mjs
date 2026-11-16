// Does Apple's notary service accept the App Store Connect API key? release.yml's custody job asks
// on Linux, before any build leg starts, what `xcrun notarytool history` asks on a Mac: the list of
// the team's earlier submissions (GET /notary/v2/submissions), with a JSON Web Token the key signs
// (ES256, as Apple's "Generating tokens for API requests" says). Plain Node, no install:
//
//   APPLE_API_KEY=<key id> APPLE_API_ISSUER=<issuer id> APPLE_API_KEY_P8=<the .p8's text> \
//     node .github/scripts/apple-notary.mjs --check
//
// It prints names and Apple's HTTP status and error titles, never the key, the token or a
// submission. Exit 0 when the service answers 200, 1 otherwise. release-apple.test.mjs tests it
// without the network.

import { createPrivateKey, sign } from "node:crypto";
import { pathToFileURL } from "node:url";

export const SUBMISSIONS_URL = "https://appstoreconnect.apple.com/notary/v2/submissions";

const base64url = (data) => Buffer.from(data).toString("base64url");

/** The names whose values are wrong in shape, from <env>: names only, never a value. */
export function credentialProblems(env) {
  const problems = [];
  if (!/^[A-Z0-9]{10}$/.test(env.APPLE_API_KEY ?? "")) {
    problems.push("APPLE_API_KEY is not a Key ID (10 capital letters and digits)");
  }
  if (!/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(env.APPLE_API_ISSUER ?? "")) {
    problems.push("APPLE_API_ISSUER is not an Issuer ID (a UUID)");
  }
  let key = null;
  try {
    key = createPrivateKey(env.APPLE_API_KEY_P8 ?? "");
  } catch {
    // Reported below.
  }
  if (key === null || key.asymmetricKeyType !== "ec" || key.asymmetricKeyDetails?.namedCurve !== "prime256v1") {
    problems.push("APPLE_API_KEY_P8 is not the text of an App Store Connect .p8 key (a P-256 private key)");
  }
  return problems;
}

/** A token for the App Store Connect API, signed with <privateKeyPem>, valid for 10 minutes. */
export function notaryToken({ keyId, issuer, privateKeyPem, now = Math.floor(Date.now() / 1000) }) {
  const header = base64url(JSON.stringify({ alg: "ES256", kid: keyId, typ: "JWT" }));
  const claims = base64url(JSON.stringify({ iss: issuer, iat: now, exp: now + 600, aud: "appstoreconnect-v1" }));
  const signature = sign("sha256", Buffer.from(`${header}.${claims}`), {
    key: createPrivateKey(privateKeyPem),
    dsaEncoding: "ieee-p1363",
  });
  return `${header}.${claims}.${base64url(signature)}`;
}

/** null when the notary service accepts the key in <env>, else what is wrong (no secret in it). */
export async function notaryProblem(env, fetchImpl = fetch) {
  const shape = credentialProblems(env);
  if (shape.length > 0) {
    return shape.join("; ");
  }
  const token = notaryToken({ keyId: env.APPLE_API_KEY, issuer: env.APPLE_API_ISSUER, privateKeyPem: env.APPLE_API_KEY_P8 });
  let response;
  try {
    response = await fetchImpl(SUBMISSIONS_URL, {
      headers: { Authorization: `Bearer ${token}` },
      signal: AbortSignal.timeout(60_000),
    });
  } catch (error) {
    return `the notary service did not answer (${error?.name ?? "error"})`;
  }
  if (response.status === 200) {
    return null;
  }
  let titles = "";
  try {
    const body = await response.json();
    titles = (body?.errors ?? [])
      .map((one) => String(one?.title ?? ""))
      .filter((title) => title !== "")
      .join("; ");
  } catch {
    // No JSON body: the status says enough.
  }
  return (
    `the notary service refused APPLE_API_KEY with HTTP ${response.status}${titles ? ` (${titles})` : ""}: ` +
    "check APPLE_API_KEY, APPLE_API_ISSUER and APPLE_API_KEY_P8 are the same key's, and the key is not revoked"
  );
}

const isMain = process.argv[1] !== undefined && import.meta.url === pathToFileURL(process.argv[1]).href;
if (isMain) {
  if (process.argv.slice(2).join(" ") !== "--check") {
    console.error("usage: node apple-notary.mjs --check (APPLE_API_KEY, APPLE_API_ISSUER, APPLE_API_KEY_P8 set)");
    process.exit(2);
  }
  const problem = await notaryProblem(process.env);
  if (problem !== null) {
    console.log(`::error::${problem}`);
    process.exit(1);
  }
  console.log("The notary service accepts APPLE_API_KEY (GET /notary/v2/submissions: 200)");
}
