# Getting started

## Install

Download the installer for your system from the
[releases page](https://github.com/custodia-secured/DataBase.Me/releases). Each release lists its
files with a `SHA256SUMS.txt`; the version is in every file name.

| System               | File                                                         |
| -------------------- | ------------------------------------------------------------ |
| Windows 10 or 11     | `DataBase.Me_<version>_x64-setup.exe`, or the `.msi`         |
| macOS, Apple Silicon | `DataBase.Me_<version>_aarch64.dmg`                          |
| macOS, Intel         | `DataBase.Me_<version>_x64.dmg`                              |
| Linux, any           | `DataBase.Me_<version>_amd64.AppImage` (needs WebKitGTK 4.1) |
| Debian, Ubuntu       | `DataBase.Me_<version>_amd64.deb`                            |
| Fedora, openSUSE     | `DataBase.Me-<version>-1.x86_64.rpm`                         |

**What the system may warn.** Release 0.1 is not code-signed: the project ships it unsigned and
publishes checksums instead. An unsigned installer is not less safe for being unsigned, but the
operating systems treat it as unknown:

- _Windows_ shows a SmartScreen page, "Windows protected your PC". Press **More info**, then
  **Run anyway**.
- _macOS_ refuses to open an app from an unidentified developer. Right-click (or Control-click)
  the app in the Finder, choose **Open**, and confirm; that is needed once. On a Mac with Apple
  Silicon, an unsigned app may also need `xattr -dr com.apple.quarantine /Applications/DataBase.Me.app`
  in Terminal.
- _Linux_ does not warn. An AppImage needs to be executable (`chmod +x`) before it runs.

**Checking a download.** The sums file lets you confirm the file arrived whole and unchanged:

```sh
sha256sum -c SHA256SUMS.txt --ignore-missing      # Linux
shasum -a 256 -c SHA256SUMS.txt --ignore-missing  # macOS
```

```powershell
Get-FileHash .\DataBase.Me_0.1.0_x64-setup.exe    # Windows: compare with the line in SHA256SUMS.txt
```

## The first launch

The app opens on a short setup:

1. **Welcome.**
2. **Where the vault goes.** The vault is one folder holding everything you put in the app. The
   default is the app's own data folder (listed below); choose another folder if you want it on a
   particular disk or in a folder you back up. The vault inside it is named `Main.dbme`.
3. **The files folder.** On by default, inside the vault: every page is also a Markdown file you
   can open in any editor, and edits there come back into the app. Turn it off, or put it in a
   folder of your choosing (an existing Obsidian vault, say).
4. **Mode.** _Simple_ is a plain notes app: pages, text, search, the files folder. _Organizer_
   adds types, fields and the Table view. The choice is a setting, changed any time.
5. **Theme.** Follow the system, light, or dark.
6. **Update checks.** Off. This version has no updater; the choice is kept for the first one that
   does, and the step says what a check would send (the app's version and your address, to the
   download host) so the choice is an informed one.

**Finish** opens the vault. Nothing in setup, and nothing after it, touches the network: see
[privacy](privacy.md).

Setup can be run again from Settings (**Set up again**). With a vault open, it keeps the vault
and skips that step.

## Where things are

| What                       | Windows                                             | macOS                                                                   | Linux                                                    |
| -------------------------- | --------------------------------------------------- | ----------------------------------------------------------------------- | -------------------------------------------------------- |
| The default vault          | `%APPDATA%\org.databaseme.desktop\vaults\Main.dbme` | `~/Library/Application Support/org.databaseme.desktop/vaults/Main.dbme` | `~/.local/share/org.databaseme.desktop/vaults/Main.dbme` |
| Settings (`settings.json`) | `%APPDATA%\org.databaseme.desktop\`                 | `~/Library/Application Support/org.databaseme.desktop/`                 | `~/.config/org.databaseme.desktop/`                      |
| The files folder (default) | `files\` inside the vault                           | `files/` inside the vault                                               | `files/` inside the vault                                |

A vault chosen during setup is wherever you put it; Settings shows the path of the open vault.
[The vault and where it lives](the-vault.md) says what is in it and how to move or copy it.

## Next

[The first ten minutes](first-ten-minutes.md) walks through writing a page, giving it structure,
finding it, editing its file outside the app, and taking a copy of everything.
