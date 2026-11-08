# When security software stops an AI tool's set-up

[हिन्दी में पढ़ें](security-software.hi.md)

The first time you choose an AI tool such as Claude or ChatGPT, Clauseo downloads its files and
sets them up on your computer. Sometimes the download works, but the set-up doesn't finish. Clauseo
then shows a message that starts like this:

> Claude was downloaded, but setting it up didn't finish.

For ChatGPT, the message names Codex, the ChatGPT tool Clauseo uses.

Check on the laptop pass: the message's final wording, in English and Hindi.

Try the steps below in order.

## Why it happens

Security software, such as Windows Security (also called Microsoft Defender) or an antivirus your
firm installed, checks new files as they arrive. While it checks them, Clauseo can't move them into
place. Now and then it holds a file for too long, or removes it because it doesn't recognise it.

Before it shows this message, Clauseo has already tried the set-up three times by itself.

## 1. Wait a minute, then choose Try again

Often the security software has finished its check by then. Wait about a minute, then choose
**Try again** under the message. Clauseo keeps the files it already downloaded and checked, so
this is usually quicker than the first time.

## 2. Restart the computer, then try again

A restart ends anything that may still be holding the files. Open Clauseo again and choose
**Try again**.

Check on the laptop pass: after a restart, whether the message and its **Try again** are still
there, or whether the lawyer has to choose the AI tool again.

## 3. Look in your security software's list of blocked files

If it still doesn't finish, your security software may have removed one of the AI tool's files.
Most security software keeps a list of what it blocked or removed, and lets you put a file back.

Clauseo keeps the AI tools' files in a folder called `agents`, inside a folder called
`ai.clauseo.desktop`. Look for a blocked file whose location includes `ai.clauseo.desktop`. The
file is often called `node.exe`, or `node` on a Mac.

### On Windows, with Windows Security

Windows may ask for an administrator's password along the way. Whoever looks after this computer
can enter it.

1. Open the Start menu, type **Windows Security** and open it.
2. Click **Virus & threat protection**.
3. Under **Current threats**, click **Protection history**.
4. Click an item from about the time Clauseo set up the AI tool. Its details show the file's
   location.
5. If the location includes `ai.clauseo.desktop`, click **Actions**, then **Restore** or
   **Allow on device**, whichever it shows.
6. Go back to Clauseo and choose **Try again**.

If no item's location includes `ai.clauseo.desktop`, Windows Security didn't block Clauseo. If you
use another antivirus, look there as below. If not, write to us (see Who to write to).

Check on the laptop pass: the exact names of these pages and buttons on Windows 11 and Windows 10,
and whether opening an item's details or Restore asks for an administrator's password.

> Screenshot to come: Windows Security's Protection history, with a Clauseo file.
<!-- img/windows-protection-history.png -->

If you use a different antivirus on Windows, open it and look for a list called something like
**Quarantine**, **History** or **Recent activity**. Look there for a file whose location includes
`ai.clauseo.desktop`, restore it, then choose **Try again** in Clauseo.

### On a Mac

The Mac's own protection doesn't keep a list you can look in. If you installed an antivirus, or
your firm did, open it and look for a list called something like **Quarantine**, **History** or
**Recent activity**. Look for a file whose location includes `ai.clauseo.desktop`, restore it, then
choose **Try again** in Clauseo.

Check on the laptop pass: whether the Mac's own protection ever removes an AI tool's files, and
what the lawyer sees if it does.

### On a computer your firm gave you

Your firm manages the security software, and it may not let you restore files yourself. If you
don't see a list, or it won't let you restore the file, send this page to your firm's IT team. The
next section tells them how to allow the AI tools' files.

## For your firm's IT team

The agent install downloads every file, checks each against its digest, then unpacks them with
`npm ci` and renames the result into place. `agent.install_setup_failed` means the downloads and
checks passed and that last part failed after three tries. On our Windows test machine, Defender
held the fresh files and the rename got `Access is denied`. Clauseo retries sharing and lock
errors in place before it counts a try.

Each user's agent files are under their Clauseo data folder (or under `CLAUSEO_DATA_DIR` when that
is set):

| System | Folder |
|---|---|
| Windows | `%LOCALAPPDATA%\ai.clauseo.desktop\agents\` |
| macOS | `~/Library/Application Support/ai.clauseo.desktop/agents/` |
| Linux | `$XDG_DATA_HOME/ai.clauseo.desktop/agents/`, by default `~/.local/share/...` |

Inside it:

- `runtime\node-v<version>-<os>-<arch>\`: the Node runtime Clauseo downloads when the computer has
  no suitable Node of its own. The program is `node.exe` on Windows, `bin/node` on macOS and Linux.
- `<agent>\install\<hash>\`: each agent's packages (`claude-acp` for Claude, `codex-acp` for
  ChatGPT), with their own programs under `node_modules`, such as Codex's `codex.exe` and its
  `rg.exe` on Windows.
- `<agent>\install\.tmp-<hash>-<n>\`: where `npm ci` unpacks a try before the rename.
- `downloads\`: the checked archives, removed once the agent is ready.

Check on the laptop pass: the programs in Claude's and Codex's packages on each system, with
their paths under `node_modules`.

**What to allow, from narrowest to widest.**

1. **Restore and allow that one detection.** In Windows Security's Protection history, or in
   Microsoft Defender for Endpoint, choose **Allow on device** for the detection, or your
   product's equivalent for that file. Scanning stays on for everything else.
2. **Allow by signer or hash, not by path.** Use Defender for Endpoint file or certificate
   indicators, or your EDR's certificate allow. To see a program's signer, open the file's
   **Properties**, then **Digital Signatures**, and allow that signer. Allow unsigned programs by
   their hash (`rg.exe` may be one). A hash changes with each new version Clauseo downloads, so a
   hash allow needs updating after an update; prefer the signer where there is one.

3. **A path exclusion only as a last resort.** An exclusion is not an allow: it turns scanning off
   for everything in that folder. These folders are user-writable, so files any program writes
   there are no longer scanned. If you need one, scope it to `agents\runtime\` and
   `agents\<agent>\install\`, never the whole data folder. Avoid process exclusions for
   `node.exe`: they stop scanning of every file it opens, including the ones the AI tool writes
   across the matter. At an administrator's prompt,
   `%LOCALAPPDATA%` is the administrator's folder, so name the user's own:
   `C:\Users\<user>\AppData\Local\ai.clauseo.desktop\agents\runtime\`.

[Clauseo for firm IT](../firm-it.md) has the rest: AppLocker and WDAC rules for this folder in
[section 3](../firm-it.md#3-applocker-and-wdac), and what to do when Windows security removes
Clauseo's own set-up program in
[section 10](../firm-it.md#10-if-windows-security-removed-clauseos-set-up-program).

## Who to write to

Open an issue at <https://github.com/clauseo-ai/clauseo-acp/issues>. Issues are public, so leave
out anything about a matter or a client.
