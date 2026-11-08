# Switching out of S mode

[हिन्दी में पढ़ें](windows-s-mode.hi.md)

Some Windows laptops come in S mode. Clauseo can't be installed or run in S mode, so you switch it
off first. It's free and takes a few minutes, but it can't be undone. If your firm gave you this
computer, ask your firm's IT team before you start. The firm may keep it in S mode, and then only
they can switch it.

## What S mode is

S mode is a locked-down setting of Windows. It runs only apps from the Microsoft Store, and it
comes switched on with some new laptops, often cheaper ones sold with Windows 11 Home.

## How to tell you're in S mode

When you open the Clauseo installer, Windows doesn't start it. Instead it shows a message that
starts:

> For security and performance, this mode of Windows only runs Microsoft-verified apps

Check on the laptop pass: the exact wording and buttons of this message on Windows 11 and
Windows 10.

> Screenshot to come: the "only runs Microsoft-verified apps" message.
<!-- img/windows-s-mode-blocked.png -->

This isn't the blue "Windows protected your PC" box, which has a **Run anyway** button. It isn't
Smart App Control either, whose message names Smart App Control.
[Installing Clauseo on Windows](windows.md) covers both.

To be sure, open the Start menu, then **Settings**, **System**, **About**. Under **Windows
specifications**, the **Edition** line says "in S mode", for example "Windows 11 Home in S mode".

Check on the laptop pass: where the edition shows "in S mode".

## Why Clauseo needs S mode off

Clauseo doesn't come from the Microsoft Store. While you work, it also runs other programs that
don't come from the Store, including the AI tools it works with. S mode blocks all of them. A
version of Clauseo in the Store wouldn't help, because those programs would still be blocked.

## Before you start

- **It's free.** Microsoft doesn't charge for switching out of S mode.
- **It takes a few minutes.** You don't lose your files or apps.
- **It can't be undone.** Once you switch out, Windows stays out of S mode for good. From then on
  your computer runs ordinary Windows, like most computers, and can install apps from outside the
  Store.
- **You need the internet, and maybe a Microsoft account.** The Microsoft Store does the switch. It
  may ask you to sign in with a Microsoft account, so have its email address and password ready.

Check on the laptop pass: whether the Store asks you to sign in.

## The clicks on Windows 11

1. Open the Start menu, then **Settings**.
2. Click **System**, then **Activation**.
3. Find the section called **Switch to Windows 11 Pro** or **Switch to Windows 11 Home**, and
   click **Go to the Store**. On some computers there is an **S mode** line instead, with an
   **Open Store** button. Click that.

   > **Careful:** don't use the link under **Upgrade your edition of Windows**. It's a paid
   > upgrade, and it keeps S mode on.

4. The Microsoft Store opens a page called **Switch out of S mode**, or something close to it.
   Click **Get**.
5. Confirm when the Store asks. If Windows asks for an administrator's password, whoever looks
   after this computer can enter it. When the page says you're done, run the Clauseo installer you
   downloaded again, and follow [Installing Clauseo on Windows](windows.md).

Check on the laptop pass: the Activation page's wording, the Store page and its confirmation, and
whether Windows asks for an administrator's password or a restart.

> Screenshot to come: Settings, Activation, with "Go to the Store" or "Open Store".
<!-- img/windows-s-mode-activation.png -->

> Screenshot to come: the Store's "Switch out of S mode" page with "Get".
<!-- img/windows-s-mode-store.png -->

## The clicks on Windows 10

1. Open the Start menu, then **Settings**.
2. Click **Update & Security**, then **Activation**.
3. Find the section called **Switch to Windows 10 Home** or **Switch to Windows 10 Pro**, and click
   **Go to the Store**.

   > **Careful:** don't use the link under **Upgrade your edition of Windows**. It's a paid
   > upgrade, and it keeps S mode on.

4. The Microsoft Store opens a page called **Switch out of S mode**, or something close to it.
   Click **Get**.
5. Confirm when the Store asks. If Windows asks for an administrator's password, whoever looks
   after this computer can enter it. When the page says you're done, run the Clauseo installer you
   downloaded again, and follow [Installing Clauseo on Windows](windows.md).

Check on the laptop pass: the Windows 10 Activation page's wording, and whether Windows asks for an
administrator's password or a restart.

> Screenshot to come: Windows 10 Settings, Activation, with "Go to the Store".
<!-- img/windows-10-s-mode-activation.png -->

## If there is no Get button, or Windows won't let you switch

**On your own computer**, you can switch. If the Store won't open or shows no **Get** button, check
that you're connected to the internet, sign in to the Store with your Microsoft account, and try
again. If it still doesn't work, ask whoever looks after this computer.

**On a computer your firm gave you**, the firm may have locked it in S mode. Then Windows won't let
you switch. Your firm's IT team can switch it for you, from the tools they use to manage the firm's
computers. Send them this page and [Clauseo for firm IT](../firm-it.md), which tells them how.

## Who to write to

Open an issue at <https://github.com/clauseo-ai/clauseo-acp/issues>. Issues are public, so leave
out anything about a matter or a client.
