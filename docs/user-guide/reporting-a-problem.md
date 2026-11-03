# Reporting a problem

Problems go to the issue tracker:
`https://github.com/custodia-secured/DataBase.Me/issues`. The app has no account, no telemetry and
no crash reporter, so nothing reaches the project unless you send it; what you send is up to you.

## The diagnostic report

Settings, under **Help**: **Copy diagnostic report** puts a short text on the clipboard (or shows
it in a box to copy by hand, where the app's window is not allowed the clipboard). Paste it into
the issue. It looks like this:

```
DataBase.Me diagnostic report
made: 2026-11-03T12:00:00.000Z
app: 0.1.0, vault format 0
system: windows x86_64
webview: WebView2 142.0.3595.65
settings: mode organizer, theme dark, update checks off
vault: 1240 objects, 37 blobs, schema 4
files folder: on
recent errors (newest first): 2026-11-03T11:58:12.411Z NotFound; 2026-11-03T11:40:03.002Z BadPath
```

It names versions, the system, the webview, your settings, counts, and the kinds of the last
twenty failures with their times. It never contains a path, a page's title, a message or anything
you wrote: a failure is recorded as its kind only, because a message can carry a path or a title.
Read it before you paste it, as with anything you send.

## What helps

- What you did, what you expected, and what happened instead, in a few lines.
- The diagnostic report.
- For a problem with a file (an import, the files folder), the file if you can share it, or a
  file you made that shows the same problem.
- For a problem with a vault you cannot share, `dbme verify <vault>` prints whether the vault's
  tables agree with its change log, and nothing from inside it.

## Security problems

A problem that could expose a vault's contents or let a file do something it should not goes to
the address in the repository's `SECURITY.md`, privately, not to the public tracker.
