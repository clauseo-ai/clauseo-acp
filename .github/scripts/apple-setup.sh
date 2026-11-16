#!/usr/bin/env bash
# Rohan's Apple Developer ID set-up for release.yml (decision 0035, "Apple Developer ID").
#
#   bash apple-setup.sh --check   # no Apple account needed: gh, the `release` environment, tools
#   bash apple-setup.sh           # the real thing, step by step, on your MacBook
#
# The real mode, after the Developer ID Application certificate exists (apple-rohan-steps.md):
#   1. finds the Developer ID Application identity in your login keychain;
#   2. checks the .p12 you export from Xcode, into a folder of its own outside Desktop and
#      Documents (which iCloud may sync), opens with its password and holds that identity;
#   3. asks for one notary set: the Apple ID with an app-specific password (recommended first),
#      or the App Store Connect team API key (.p8, Key ID, Issuer ID);
#   4. checks it with `xcrun notarytool`: the app-specific password is pasted once, and that
#      same value is the one notarytool checks and the one GitHub gets (notarytool takes it on
#      its command line for that one check, on your Mac only: it has no stdin option); and the
#      identity with a timestamped codesign of a scratch file;
#   5. sets the six secrets (the certificate trio and the chosen set) in clauseo-ai/clauseo-acp's
#      `release` environment with `gh secret set`, each value from stdin or a file, deletes the
#      other set's if present (release.yml refuses both), then lists the names;
#   6. copies the .p12 (and the .p8) to the USB that holds the updater key, checks the copies and
#      deletes the local ones. The .p12's password goes in your password manager only, never on
#      the USB beside the file it opens.
# The Team ID must also be committed in crates/desktop/release/apple-team-id (release.yml
# refuses to sign without it); the script says so, and stops if that file pins another team.
# It never prints a secret: passwords are read without echo, files go straight to gh's stdin.
# Nothing is set until you say yes. Safe to stop with Ctrl-C at any point; the temporary
# keychain and folder are removed on exit, and the clipboard is cleared. It runs only by hand,
# in a terminal: never under CI.

set -euo pipefail

# By hand only: never in CI, never without a terminal to type into.
if [ -n "${CI:-}" ] || [ -n "${GITHUB_ACTIONS:-}" ]; then
  printf 'Stopped: apple-setup.sh runs by hand on your Mac, never under CI\n' >&2
  exit 1
fi
if ! [ -t 0 ] || ! [ -t 1 ]; then
  printf 'Stopped: apple-setup.sh needs a terminal (run it in Terminal, not through a pipe)\n' >&2
  exit 1
fi

REPO=clauseo-ai/clauseo-acp
ENVIRONMENT=release
CERTIFICATE=(APPLE_CERTIFICATE APPLE_CERTIFICATE_PASSWORD APPLE_SIGNING_IDENTITY)
APPLE_ID_SET=(APPLE_ID APPLE_PASSWORD APPLE_TEAM_ID)
API_KEY_SET=(APPLE_API_KEY APPLE_API_ISSUER APPLE_API_KEY_P8)
# The committed Team ID release.yml signs with (decision 0035): empty until the enrolment.
PIN_FILE=$(cd "$(dirname "$0")/../.." && pwd)/crates/desktop/release/apple-team-id
pinned_team() { if [ -f "$PIN_FILE" ]; then sed 's/#.*//' "$PIN_FILE" | tr -d '[:space:]'; fi; }

say() { printf '\n== %s\n' "$*"; }
ok() { printf '  ok: %s\n' "$*"; }
bad() { printf '  NOT OK: %s\n' "$*"; }
die() { printf '\nStopped: %s\n' "$*" >&2; exit 1; }
ask() { # $1 prompt, $2 default; the answer on stdout
  local answer
  read -r -p "$1${2:+ [$2]}: " answer
  printf '%s' "${answer:-$2}"
}
yes_or_stop() { [ "$(ask "$1 (yes/no)" no)" = yes ] || die "you said no"; }

# ---- --check: what works without an Apple account ----
check() {
  local failed=0
  say "Tools"
  for tool in gh base64 openssl security xcrun codesign; do
    if command -v "$tool" > /dev/null; then ok "$tool"; else bad "$tool is missing"; failed=1; fi
  done
  if command -v xcrun > /dev/null; then
    if xcrun notarytool --version > /dev/null 2>&1; then
      ok "xcrun notarytool $(xcrun notarytool --version 2>/dev/null | head -1)"
    else
      bad "xcrun notarytool does not run: install Xcode or the Command Line Tools (xcode-select --install)"
      failed=1
    fi
  fi
  [ "$(uname -s)" = Darwin ] && ok "macOS $(sw_vers -productVersion 2>/dev/null)" ||
    { bad "this is not macOS: the real mode needs your MacBook"; failed=1; }

  say "GitHub"
  if command -v gh > /dev/null && gh auth status > /dev/null 2>&1; then
    ok "gh is logged in as $(gh api user --jq .login 2>/dev/null)"
    if gh api "repos/$REPO/environments/$ENVIRONMENT" --jq .name > /dev/null 2>&1; then
      ok "gh can see $REPO's \`$ENVIRONMENT\` environment"
    else
      bad "gh cannot see $REPO's \`$ENVIRONMENT\` environment (are you an admin of clauseo-ai?)"
      failed=1
    fi
    local names
    if names=$(gh secret list --env "$ENVIRONMENT" -R "$REPO" --json name --jq '.[].name' 2>/dev/null); then
      ok "gh can list the environment's secret names (values are never readable)"
      local in_apple_id=0 in_api_key=0
      for name in "${CERTIFICATE[@]}" "${APPLE_ID_SET[@]}" "${API_KEY_SET[@]}"; do
        grep -qx "$name" <<< "$names" && printf '     %s: set\n' "$name" || printf '     %s: not set\n' "$name"
      done
      for name in "${APPLE_ID_SET[@]}"; do grep -qx "$name" <<< "$names" && in_apple_id=1; done
      for name in "${API_KEY_SET[@]}"; do grep -qx "$name" <<< "$names" && in_api_key=1; done
      if [ "$in_apple_id" = 1 ] && [ "$in_api_key" = 1 ]; then
        bad "both notary sets are (partly) set: release.yml's custody refuses that; the real mode deletes the set you don't pick"
        failed=1
      fi
    else
      bad "gh cannot list the environment's secrets (it needs admin on $REPO)"
      failed=1
    fi
    # Repository-level Apple secrets reach every mode's custody (has_apple): keep them out.
    if names=$(gh secret list -R "$REPO" --json name --jq '.[].name' 2>/dev/null); then
      if grep -q '^APPLE_' <<< "$names"; then
        bad "the repository itself has $(grep '^APPLE_' <<< "$names" | tr '\n' ' '); keep Apple secrets in \`$ENVIRONMENT\` only"
        failed=1
      else
        ok "no Apple secret at the repository level"
      fi
    fi
  else
    bad "gh is not logged in: run gh auth login"
    failed=1
  fi

  say "The committed Team ID"
  if [ ! -f "$PIN_FILE" ]; then
    bad "no $PIN_FILE: run this from a checkout of $REPO"
    failed=1
  elif [ -z "$(pinned_team)" ]; then
    ok "crates/desktop/release/apple-team-id is empty: after the real mode, a one-line PR sets it"
  else
    ok "crates/desktop/release/apple-team-id pins $(pinned_team)"
  fi

  say "Result"
  if [ "$failed" = 0 ]; then echo "  Everything the real mode needs is here."; else echo "  Fix the NOT OK lines first."; fi
  return "$failed"
}

if [ "${1:-}" = --check ]; then
  check
  exit $?
fi
[ $# = 0 ] || die "usage: bash apple-setup.sh [--check]"

# ---- The real mode ----
[ "$(uname -s)" = Darwin ] || die "run this on your MacBook"
check || die "the checks above must pass first (bash apple-setup.sh --check)"

# A folder of its own in your home folder, which iCloud does not sync (Desktop and Documents
# may be); the .p12 is exported into it, and the whole folder goes when the script ends.
work=$(mktemp -d "$HOME/clauseo-apple-setup.XXXXXX")
chmod 700 "$work"
probe_keychain=$work/probe.keychain-db
cleanup() {
  security delete-keychain "$probe_keychain" 2> /dev/null || true
  rm -rf "$work"
  unset p12_password asp
  # The app-specific password was pasted from the clipboard.
  pbcopy < /dev/null 2> /dev/null && printf '\n  The clipboard is cleared; %s is deleted.\n' "$work"
}
trap cleanup EXIT

say "1. The Developer ID Application identity in your login keychain"
# macOS's /bin/bash is 3.2: no mapfile.
identities=()
while IFS= read -r line; do identities+=("$line"); done < <(security find-identity -v -p codesigning |
  sed -n 's/^ *[0-9]*) [0-9A-F]\{40\} "\(Developer ID Application: .* ([A-Z0-9]\{10\})\)"$/\1/p' | sort -u)
case "${#identities[@]}" in
  0) die "no valid Developer ID Application identity. Make it first: apple-rohan-steps.md, step 2" ;;
  1) identity=${identities[0]} ;;
  *)
    echo "  More than one:"
    for i in "${!identities[@]}"; do echo "    $((i + 1))) ${identities[$i]}"; done
    pick=$(ask "  Which one" 1)
    [[ "$pick" =~ ^[0-9]{1,3}$ ]] && [ "$pick" -ge 1 ] && [ "$pick" -le "${#identities[@]}" ] ||
      die "answer a number from 1 to ${#identities[@]}"
    identity=${identities[$((pick - 1))]}
    ;;
esac
team=$(sed -n 's/.*(\([A-Z0-9]\{10\}\))$/\1/p' <<< "$identity")
ok "found \"$identity\" (Team ID $team)"
pinned=$(pinned_team)
[ -z "$pinned" ] || [ "$pinned" = "$team" ] ||
  die "this identity's Team ID is $team, but crates/desktop/release/apple-team-id pins $pinned: release.yml would refuse it"
yes_or_stop "  Is this your Developer ID, the one releases should be signed with?"

say "2. Export it as a .p12 (Xcode does it with the private key)"
cat << EOF
  In Xcode: Xcode > Settings… > Accounts, select your team, click Manage Certificates….
  Control-click the "Developer ID Application" certificate (the one ending ($team)),
  choose Export Certificate, and save it as DeveloperID.p12 in this folder (in the save
  dialog press Command-Shift-G and paste it), not on the Desktop, which iCloud may sync:
    $work
  Type a new, strong password twice, and keep it in your password manager: it is not
  backed up anywhere else. Then come back here.
EOF
p12=$(ask "  Path of the .p12" "$work/DeveloperID.p12")
[ -f "$p12" ] || die "no file at $p12"
case "$p12" in
  "$work"/*) ;;
  *) echo "  Note: $p12 is outside $work; Desktop and Documents may sync to iCloud." ;;
esac
read -r -s -p "  The .p12's password (not shown): " p12_password; echo
read -r -s -p "  Again: " again; echo
[ "$p12_password" = "$again" ] || die "the two passwords differ"
unset again
# Opened the way the runner opens it: imported into a throwaway keychain.
probe_password=$(openssl rand -hex 16)
security create-keychain -p "$probe_password" "$probe_keychain"
security unlock-keychain -p "$probe_password" "$probe_keychain"
security import "$p12" -k "$probe_keychain" -f pkcs12 -P "$p12_password" > /dev/null 2>&1 ||
  die "the .p12 does not open with that password"
security find-identity -p codesigning "$probe_keychain" | grep -qF "\"$identity\"" ||
  die "the .p12 does not hold \"$identity\" with its private key"
leaves=$(security find-certificate -a -c "Developer ID Application:" -p "$probe_keychain" | grep -c 'BEGIN CERTIFICATE' || true)
[ "$leaves" = 1 ] || die "the .p12 holds $leaves Developer ID Application certificates; export only the one"
end=$(security find-certificate -c "$identity" -p "$probe_keychain" | openssl x509 -noout -enddate | cut -d= -f2)
security delete-keychain "$probe_keychain"
ok "the .p12 opens with its password and holds the identity and its key (valid until $end)"

say "3. The notary credentials: the Apple ID (recommended now) or the API key"
cat << EOF
  1) Apple ID + app-specific password: works the day the membership is active.
  2) App Store Connect team API key (.p8): needs Apple to approve API access first.
  One set only: release.yml refuses both, or part of one.
EOF
case "$(ask "  Which" 1)" in
  1) notary=apple_id ;;
  2) notary=api_key ;;
  *) die "answer 1 or 2" ;;
esac
p8=""
if [ "$notary" = apple_id ]; then
  apple_id=$(ask "  The Apple Account (email) you enrolled with" "")
  [[ "$apple_id" == ?*@?*.?* ]] || die "that is not an email address"
  cat << EOF
  Make an app-specific password for it:
    account.apple.com > sign in > Sign-In and Security > App-Specific Passwords >
    Generate an app-specific password (or +), name it "Clauseo notarytool", Create.
  Apple shows it once, as xxxx-xxxx-xxxx-xxxx. Copy it, then paste it here (not shown). This
  one paste is what notarytool checks and what GitHub gets.
EOF
  read -r -s -p "  App-specific password: " asp; echo
  [[ "$asp" =~ ^[a-z]{4}-[a-z]{4}-[a-z]{4}-[a-z]{4}$ ]] ||
    die "that is not an app-specific password (four groups of four letters); never your Apple Account password"
else
  p8_default=$(ls -t "$HOME"/Downloads/AuthKey_*.p8 2> /dev/null | head -1 || true)
  p8=$(ask "  Path of the AuthKey_<KEYID>.p8 you downloaded" "$p8_default")
  [ -f "$p8" ] || die "no file at $p8"
  key_id_default=$(basename "$p8" | sed -n 's/^AuthKey_\([A-Z0-9]\{10\}\)\.p8$/\1/p')
  key_id=$(ask "  Key ID (App Store Connect, Team Keys, the key's row)" "$key_id_default")
  [[ "$key_id" =~ ^[A-Z0-9]{10}$ ]] || die "a Key ID is 10 capital letters and digits"
  read -r -p "  Issuer ID (at the top of the Team Keys page; paste it): " issuer
  [[ "$issuer" =~ ^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$ ]] ||
    die "an Issuer ID is a UUID (8-4-4-4-12 hex digits)"
fi

say "4. Proof on your own Mac"
if [ "$notary" = apple_id ]; then
  # The one value pasted above, so what GitHub gets is what notarytool checked. notarytool has
  # no stdin option, so it takes it on its command line, for this one call on your own Mac.
  # store-credentials checks it with Apple and keeps it in your login keychain as the profile
  # "clauseo-notary" (handy for notarytool by hand later).
  xcrun notarytool store-credentials clauseo-notary --apple-id "$apple_id" --team-id "$team" --password "$asp" ||
    die "notarytool refused the Apple ID, the password or the Team ID $team"
  xcrun notarytool history --keychain-profile clauseo-notary > /dev/null 2> "$work/notary.err" ||
    { sed 's/^/    /' "$work/notary.err"; die "notarytool history refused the stored credentials"; }
  ok "the notary service accepts $apple_id with Team ID $team (xcrun notarytool history)"
else
  if xcrun notarytool history --key "$p8" --key-id "$key_id" --issuer "$issuer" > /dev/null 2> "$work/notary.err"; then
    ok "the notary service accepts the API key (xcrun notarytool history)"
  else
    echo "  notarytool says:"; sed 's/^/    /' "$work/notary.err"
    die "the notary service refused the key: check the Key ID, the Issuer ID and that it is a Team key"
  fi
fi
cp /usr/bin/true "$work/probe"
codesign --force -s "$identity" --options runtime --timestamp "$work/probe" 2> "$work/codesign.err" ||
  { sed 's/^/    /' "$work/codesign.err"; die "codesign could not sign with \"$identity\""; }
info=$(codesign -dvvv "$work/probe" 2>&1)
grep -qx "TeamIdentifier=$team" <<< "$info" && grep -q '^Timestamp=' <<< "$info" ||
  die "the scratch signature has no Team ID $team or no secure timestamp"
ok "codesign signs with the identity, Team ID $team, with Apple's secure timestamp"

say "5. The six secrets in $REPO's \`$ENVIRONMENT\` environment"
if [ "$notary" = apple_id ]; then
  mine=(APPLE_ID APPLE_PASSWORD APPLE_TEAM_ID) other=(APPLE_API_KEY APPLE_API_ISSUER APPLE_API_KEY_P8)
else
  mine=(APPLE_API_KEY APPLE_API_ISSUER APPLE_API_KEY_P8) other=(APPLE_ID APPLE_PASSWORD APPLE_TEAM_ID)
fi
echo "  APPLE_CERTIFICATE (the .p12, base64), APPLE_CERTIFICATE_PASSWORD, APPLE_SIGNING_IDENTITY,"
echo "  ${mine[*]}. Existing values are replaced."
existing=$(gh secret list --env "$ENVIRONMENT" -R "$REPO" --json name --jq '.[].name')
stale=()
for name in "${other[@]}"; do grep -qx "$name" <<< "$existing" && stale+=("$name"); done
[ "${#stale[@]}" = 0 ] || echo "  The other set's ${stale[*]} will be deleted: release.yml refuses both sets."
yes_or_stop "  Set them now?"
set_secret() { gh secret set "$1" --env "$ENVIRONMENT" -R "$REPO" > /dev/null && ok "$1 set"; }
base64 -i "$p12" | tr -d '\n' | set_secret APPLE_CERTIFICATE
printf '%s' "$p12_password" | set_secret APPLE_CERTIFICATE_PASSWORD
printf '%s' "$identity" | set_secret APPLE_SIGNING_IDENTITY
if [ "$notary" = apple_id ]; then
  printf '%s' "$apple_id" | set_secret APPLE_ID
  printf '%s' "$asp" | set_secret APPLE_PASSWORD
  printf '%s' "$team" | set_secret APPLE_TEAM_ID
  unset asp
else
  printf '%s' "$key_id" | set_secret APPLE_API_KEY
  printf '%s' "$issuer" | set_secret APPLE_API_ISSUER
  set_secret APPLE_API_KEY_P8 < "$p8"
fi
for name in ${stale[@]+"${stale[@]}"}; do
  gh secret delete "$name" --env "$ENVIRONMENT" -R "$REPO" && ok "$name deleted"
done
echo "  The environment's secret names now:"
gh secret list --env "$ENVIRONMENT" -R "$REPO" --json name,updatedAt --jq '.[] | "    \(.name)  (\(.updatedAt))"'

say "6. Back up to the USB with the updater key, then delete the local copies"
usb=$(ask "  The USB's folder (for example /Volumes/KEYS)" "")
while [ ! -d "$usb" ]; do
  usb=$(ask "  No folder at \"$usb\". Plug the USB in and type its folder (empty: stop; the .p12 is then deleted, export it again later)" "")
  [ -n "$usb" ] || die "stopped before the backup; the secrets are set on GitHub"
done
dest=$usb/clauseo-apple-$(date +%Y-%m-%d)
mkdir -p "$dest"
# The .p12 only: its password stays in your password manager, never beside the file it opens.
cp "$p12" "$dest/DeveloperID.p12"
cmp -s "$p12" "$dest/DeveloperID.p12" || die "the copy on the USB differs from the .p12; nothing deleted"
if [ -n "$p8" ]; then
  cp "$p8" "$dest/$(basename "$p8")"
  cmp -s "$p8" "$dest/$(basename "$p8")" || die "the copy on the USB differs from the .p8; nothing deleted"
  notary_line="$(basename "$p8"): App Store Connect team API key $key_id (issuer in the release
environment's APPLE_API_ISSUER). Apple lets you download a .p8 only once; this is the only copy."
else
  notary_line="Notarisation: $apple_id's app-specific password \"Clauseo notarytool\", only in GitHub
(APPLE_PASSWORD). Not backed up on purpose: revoke it at account.apple.com and make a new one."
fi
cat > "$dest/README.txt" << EOF
Clauseo's Apple Developer ID, backed up $(date +%Y-%m-%d) by apple-setup.sh.
DeveloperID.p12: "$identity", valid until $end. Its password: in the password manager only.
$notary_line
GitHub: the APPLE_* secrets of $REPO's \`$ENVIRONMENT\` environment.
EOF
ok "copied to $dest and checked"
# The .p12 in the set-up folder goes with it on exit; one saved elsewhere, and the .p8, on a yes.
local_copies=()
case "$p12" in "$work"/*) ;; *) local_copies+=("$p12") ;; esac
[ -z "$p8" ] || local_copies+=("$p8")
if [ "${#local_copies[@]}" -gt 0 ]; then
  yes_or_stop "  Delete the local ${local_copies[*]}?"
  rm -f "${local_copies[@]}"
fi
rm -f "$p12"
ok "deleted. Empty the Bin if Finder moved anything there; eject the USB"
echo
if [ "$(pinned_team)" != "$team" ]; then
  echo "Last: open a one-line PR setting crates/desktop/release/apple-team-id to $team."
  echo "release.yml refuses to sign with the Developer ID until that file holds it."
fi
echo "Done. The next v* release's macOS legs sign with the Developer ID and notarise."
echo "Delete the repository variable CLAUSEO_UNSIGNED_BETA once that release is out."
