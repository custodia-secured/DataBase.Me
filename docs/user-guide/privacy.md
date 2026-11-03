# Privacy: the no-network promise, and how to check it

DataBase.Me makes no network request on its own. Not on first launch, not while you work, not in
the background. Your vault is a folder on your disk, and the app reads and writes that folder and
nothing else. There is no account, no telemetry, no crash reporting, no update check unless you
turn one on in a version that has an updater (0.1 has none; the setting is kept for the first
version that does, and it is off unless you turn it on).

This page says what the app does, how that is tested on every change, and how you can check it
yourself on your own machine, without trusting this page.

## What holds it

- **The interface is in the app.** Every script, style, font and icon ships inside the program.
  Nothing is fetched from a content network when the window opens.
- **The window's Content Security Policy forbids the rest.** The page inside the app may talk to
  the app's own bridge and to nothing else (`connect-src` names only the bridge; images come from
  the bundle, from `data:` or from `blob:`). A library that tried to reach out would be refused by
  the webview before the request left. The policy is in `apps/desktop/src-tauri/tauri.conf.json`
  and is explained in ADR-0010.
- **No updater, no opener.** 0.1 carries no update plugin and no "open in browser" plugin. Help
  shows the documentation and issue-tracker addresses as text to copy.
- **The vault stays where you put it.** Nothing about your screen, your habits or your machine is
  written into the vault, so a vault you copy to another computer carries nothing it should not.

## How it is tested on every change

On every push, the continuous-integration job on Linux builds the release app, starts it on a
virtual display with a fresh configuration folder (so the defaults apply, as on a first launch)
and traces every socket call the app and its webview make, with `strace -f -e trace=network`.
A script, `scripts/network-probe.mjs`, reads the trace and lists every `connect`, `sendto` and
`sendmsg` with an internet address. Loopback addresses and Unix sockets (the display, D-Bus, the
webview's own helper processes, the app's bridge) are the app talking to the machine it runs on.
Any other address fails the job. The trace is kept as a build artifact named `network-trace` for
thirty days, so the record can be read, not only the verdict.

The end-to-end tests add a second check on every run: the test harness fails any test during
which the page asked for anything outside the test server's own address. That catches a stray
`fetch` in the interface the day it is written.

Windows (WebView2) and macOS (WKWebView) cannot be traced that way on a shared build machine, so
they are checked by hand once per release with the operating system's own tools, by the procedure
below, and the result is written into the release notes.

## Check it yourself

The checks below watch the whole machine, so close other programs that use the network first, or
read only the rows that name DataBase.Me. Make the configuration fresh so that the launch is a
first launch: move `settings.json` out of the app's configuration folder (listed at the end of
this page) for the duration, or make a new user account on the machine.

### Windows 11

1. Open **Resource Monitor** (press the Windows key, type `resmon`, press Enter) and open the
   **Network** tab. Expand **TCP Connections** and **Network Activity**.
2. Start DataBase.Me and go through onboarding with the defaults.
3. Watch the two lists for a minute. Rows whose **Image** is `dbme-desktop.exe` or
   `msedgewebview2.exe` with a **Remote Address** that is not `127.0.0.1` or `::1` would be
   requests. There should be none. WebView2's own helper processes appear under
   `msedgewebview2.exe`; they are part of the app's window and are held to the same promise.
4. For a record, run this in PowerShell while the app is open:

   ```powershell
   Get-NetTCPConnection | Where-Object { $_.OwningProcess -in (Get-Process dbme-desktop, msedgewebview2 -ErrorAction SilentlyContinue).Id } | Select-Object RemoteAddress, RemotePort, State
   ```

   An empty result, or rows with only loopback addresses, is a pass.

5. For a packet-level record, **Packet Monitor** ships with Windows: `pktmon start --capture`
   before the launch, `pktmon stop` after, and `pktmon etl2txt PktMon.etl` to read it. Filter the
   text by the process names above.

### macOS

1. Open **Terminal** and start a trace of connections by process, before launching the app:

   ```sh
   sudo nettop -P -L 0 -J bytes_in,bytes_out
   ```

   `nettop` lists every process with a socket and the bytes it moved. Leave it running.

2. Start DataBase.Me and go through onboarding with the defaults.
3. Rows named `DataBase.Me` or `com.apple.WebKit.Networking` (the webview's network process,
   which the app's window owns) should show no bytes in or out. Stop `nettop` with Ctrl-C.
4. For a packet-level record, capture the interface during the launch and read it afterwards:

   ```sh
   sudo tcpdump -i any -w ~/Desktop/dbme-launch.pcap
   ```

   Then open the capture in Wireshark, or print it with `tcpdump -r`, and look for anything that
   is not local traffic from other programs. There should be nothing from the app.

### Linux

The same trace the build machine runs, on your own machine:

```sh
strace -f -o /tmp/dbme-trace.txt -e trace=network ./DataBase.Me_0.1.0_amd64.AppImage
```

Go through onboarding, close the app, then read the trace:

```sh
grep -E 'connect|sendto|sendmsg' /tmp/dbme-trace.txt | grep -v AF_UNIX
```

Every line left should name `127.0.0.1`, `::1` or no address at all. The repository's
`scripts/network-probe.mjs` reads the same file and prints the verdict, if you have Node.js.

## What would change this

Two features on the roadmap would make a request, and both are opt-in, off by default, and
explained in the setting that turns them on: an update check (the app's version and your address
would reach the download host), and the AI features of a later milestone, which the brief orders
to be asked about in onboarding and to default to off. Neither exists in 0.1. When one ships, the
network test above changes with it: the default launch must still make no request, and the
feature's request is tested on its own.

## Where the configuration lives

- Windows: `%APPDATA%\org.databaseme.desktop\`
- macOS: `~/Library/Application Support/org.databaseme.desktop/`
- Linux: `~/.config/org.databaseme.desktop/`

`settings.json` there holds the mode, the theme, the update-check choice, the last vault's path and
whether onboarding ran. The vault itself is wherever onboarding put it: by default, a `Main.dbme`
folder in the app's data folder, or the folder you chose.
