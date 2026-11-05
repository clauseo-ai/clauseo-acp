# Installing Clauseo on Windows

Windows warns you when you install Clauseo. This page says why, what the warning means, how to
check your download, and the clicks to get past it.

## Why the warning appears

Clauseo is a small open-source project with no funding yet. Windows trusts an installer more when
it carries a digital signature, a seal bought from a certificate company that names who made the
file. Clauseo's installer has no signature, so Windows can't say who made it and warns you.

Signing certificates cost money every year. We will buy one once Clauseo has users. Until then,
every version shows this warning.

## What the warning means

It means Windows doesn't know who published the file and hasn't seen it often. That's all.

It doesn't mean Windows found a virus or anything wrong in the file. It also doesn't prove the
file is safe. That is your call, and the next section gives you a way to check.

## Check the download (optional)

For the careful. It takes about two minutes.

1. Download Clauseo only from its GitHub release page:
   <https://github.com/clauseo-ai/clauseo-acp/releases>. Each release is built there by GitHub
   from the public source code.
2. On the same release, open `SHA256SUMS.txt`. Each line is a long code, then a file name. Find
   the line for the file you downloaded.
3. Open the Start menu, type **PowerShell** and open it.
4. Type `cd Downloads` and press Enter.
5. Type this line and press Enter:

   ```powershell
   Get-FileHash .\Clauseo* | Format-List
   ```

6. PowerShell prints each Clauseo file in your Downloads folder, with a long code after **Hash**.
   It must match the code in `SHA256SUMS.txt` exactly. Capital and small letters don't matter.

If the codes match, your file is the one GitHub built. If they don't, don't open the file. Delete
it and write to us (see the end of this page).

The source code is public at <https://github.com/clauseo-ai/clauseo-acp>, for you or your IT team
to read.

> Screenshot to come: PowerShell printing the code, beside the matching line of `SHA256SUMS.txt`.
<!-- img/windows-hash-check.png -->

## The clicks

### 1. Keep the file in your browser

Your browser may hold the download back. If it doesn't, go on to step 2.

**Microsoft Edge** may say the file "isn't commonly downloaded" (check on the laptop pass).

1. In the downloads list, point at the Clauseo file and click the three dots (**…**).
2. Click **Keep**.
3. If Edge asks again, click **Show more**, then **Keep anyway**.

> Screenshot to come: Edge's download bar with "Keep".
<!-- img/windows-edge-keep.png -->

> Screenshot to come: Edge's "Show more" with "Keep anyway".
<!-- img/windows-edge-keep-anyway.png -->

**Google Chrome** may call the file suspicious or unverified (check on the laptop pass).

1. Open the downloads list: the arrow at the top right, or Ctrl+J.
2. Next to the Clauseo file, click **Keep** or **Download suspicious file**, whichever it shows.

> Screenshot to come: Chrome's downloads list with "Download suspicious file".
<!-- img/windows-chrome-keep.png -->

### 2. "Windows protected your PC"

1. Open the file you downloaded.
2. A blue box says **Windows protected your PC**. It shows only **Don't run**.
3. Click **More info**, the small link under the text.
4. The box now shows "Publisher: Unknown publisher" and a **Run anyway** button. Click
   **Run anyway**.

> Screenshot to come: "Windows protected your PC" before "More info".
<!-- img/windows-smartscreen.png -->

> Screenshot to come: the same box with "Run anyway".
<!-- img/windows-smartscreen-run.png -->

### 3. The per-machine installer only

The usual installer, `Clauseo_<version>_x64-setup.exe`, installs for you alone and asks no
further question. Skip this step.

The per-machine installer, `Clauseo_<version>_x64-machine-setup.exe`, is the one firms use. It
installs for everyone on the computer, so Windows asks:

> Do you want to allow this app from an unknown publisher to make changes to your device?

It shows "Publisher: Unknown". Click **Yes**. You need an administrator's password.

> Screenshot to come: the UAC prompt with "Publisher: Unknown" and "Yes".
<!-- img/windows-uac.png -->

### 4. The installer

The Clauseo installer opens. Follow its steps.

## If there is no "Run anyway"

On a work computer, your firm's IT team may have switched it off. Send them this page and
[Clauseo for firm IT](../firm-it.md).

Some Windows 11 computers have **Smart App Control** on. It blocks unsigned apps with no
"Run anyway" and no way to allow one app. You can turn it off in Windows Security, App & browser
control, Smart App Control settings (check on the laptop pass). That lowers the computer's
protection, so on a work computer ask your IT team first.

## After install

Clauseo is in the Start menu. Open it from there.

The warnings come again for each new version you download by hand.

If something goes wrong, Clauseo keeps a log on your computer:
`%LOCALAPPDATA%\ai.clauseo.desktop\logs\host.log`. It holds Clauseo's own messages only, never
your matters' content or file names, so you can send it to us.

## Who to write to

Open an issue at <https://github.com/clauseo-ai/clauseo-acp/issues>. Issues are public, so leave
out anything about a matter or a client.
