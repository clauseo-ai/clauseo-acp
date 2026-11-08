# Installing Clauseo on a Mac

For now, your Mac refuses to open Clauseo the first time. This page says why, what it means, how
to check your download, and the clicks to open it.

## Why the Mac refuses

Clauseo is a small open-source project with no funding yet. Apple lets an app open without a
question once Apple has checked it, a check called notarisation, done under a paid Apple developer
account. Clauseo's builds aren't notarised yet. They carry only their own seal, which says the
app hasn't changed since it was built but not who built it.

So macOS can't check who made Clauseo, and blocks it until you say yes.

This is temporary. Release builds will be notarised by Apple and signed with a Developer ID in the
maintainer's name, Rohan Shiralkar, once it's set up. Then this page shrinks to "open it as usual".

## What the message means

macOS may say Clauseo "can't be opened", that "Apple could not verify" it, or that it comes "from
an unidentified developer". It means Apple hasn't checked this app. That's all.

It doesn't mean macOS found malware or anything wrong in the app. It also doesn't prove the app
is harmless. That is your call, and the next section gives you a way to check.

## Check the download, if you want to

For the careful. It takes about two minutes.

1. Download Clauseo only from its GitHub release page:
   <https://github.com/clauseo-ai/clauseo-acp/releases>. Each release is built there by GitHub
   from the public source code. Most Macs need the file ending `_aarch64.dmg`; an older Intel Mac
   needs `_x64.dmg`.
2. On the same release, open `SHA256SUMS.txt`. Each line is a long code, then a file name. Find
   the line for the file you downloaded.
3. Open Terminal: press Command and Space, type **Terminal**, press Return.
4. Type this, with a space at the end, but don't press Return yet:

   ```sh
   shasum -a 256
   ```

5. Drag the downloaded file from Finder into the Terminal window. Its name appears. Press Return.
6. Terminal prints a long code, then the file's name. The code must match the one in
   `SHA256SUMS.txt` exactly.

If the codes match, your file is the one listed on the release page. If they don't, don't open
the file. Delete it and write to us as the last section says.

The source code is public at <https://github.com/clauseo-ai/clauseo-acp>, for you or your IT team
to read.

> Screenshot to come: Terminal printing the code, beside the matching line of `SHA256SUMS.txt`.
<!-- img/macos-hash-check.png -->

## The clicks

These are for macOS 14 Sonoma, 15 Sequoia and 26 Tahoe.

Check on the laptop pass: that macOS 26 behaves as 15 does.

### 1. Install

1. Open the downloaded `.dmg` file.
2. Drag **Clauseo** onto **Applications**.

### 2. Open it once

1. Open **Clauseo** from Applications.
2. macOS says it can't be opened. Click **Done** or **OK**, not the button that moves it to the Bin.

Check on the laptop pass: the message and its buttons on 14, 15 and 26.

> Screenshot to come: the message that Clauseo can't be opened.
<!-- img/macos-not-opened.png -->

### 3. Allow it in System Settings

1. Open the Apple menu, then **System Settings**.
2. Click **Privacy & Security** in the sidebar.
3. Scroll down to **Security**. It says Clauseo was blocked. Click **Open Anyway**.
4. Confirm with your password or Touch ID.
5. If macOS asks once more, click **Open Anyway**.

Check on the laptop pass: whether that last question appears.

Clauseo opens. From then on it opens as usual, until the next version.

> Screenshot to come: Privacy & Security with "Open Anyway".
<!-- img/macos-open-anyway.png -->

> Screenshot to come: the last question, with "Open Anyway".
<!-- img/macos-open-anyway-confirm.png -->

On macOS 15 and later, right-clicking the app and choosing **Open** no longer gets past this
block, so use System Settings as above. On macOS 14 the right-click way still works.

Check on the laptop pass: right-click, Open on macOS 14.

### If macOS says Clauseo "is damaged"

This message can appear for an app that isn't notarised, even when nothing is wrong with it.
If your checksum matched, the file is the one listed on the release page.

Check on the laptop pass: whether these builds show this message, and without **Open Anyway**.

There may be no **Open Anyway** in this case. The way round is one line in Terminal. It removes
the "downloaded from the internet" mark from Clauseo alone:

```sh
xattr -dr com.apple.quarantine /Applications/Clauseo.app
```

Then open Clauseo again. On a work Mac, ask your IT team before you do this.

## After install

Clauseo is in your Applications folder. You can keep it in the Dock.

If something goes wrong, Clauseo keeps a log on your Mac, in
`~/Library/Application Support/ai.clauseo.desktop/logs/host.log`. It holds Clauseo's own messages
only, never your matters' content or file names, so you can send it to us.

## Who to write to

Open an issue at <https://github.com/clauseo-ai/clauseo-acp/issues>. Issues are public, so leave
out anything about a matter or a client.
